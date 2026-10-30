use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ash::vk;

use truvis_asset::handle::TextureBytes;
use truvis_gfx::commands::barrier::GfxBufferBarrier;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::commands::command_pool::GfxCommandPool;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::commands::submit_info::GfxSubmitInfo;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxQueueCtx, GfxResourceCtx};
use truvis_gfx::resources::buffer::GfxBuffer;
use truvis_gfx::resources::image::{GfxImage, GfxImageCreateInfo};
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_world::guid_new_type::TextureAssetHandle;

use crate::render_world::sky_distribution_builder::SkyDistributionBuild;

/// 已完成的 scene texture image 上传。
///
/// timeline 完成后 image 所有权从共享 queue 移交给 `GpuTextureStore`；在此之前
/// image 不允许注册到 `GfxResourceRegistry` 或 bindless。
pub(crate) struct CompletedTextureUpload {
    pub(crate) handle: TextureAssetHandle,
    pub(crate) image: GfxImage,
}

/// timeline 已确认完成的 sky distribution buffer。
///
/// `GpuSkyStore` 必须再次校验 request id 与 texture handle，匹配后才可把 device
/// address 发布到 scene root；stale completion 则在这里之后立即安全销毁。
pub(crate) struct CompletedSkyDistributionUpload {
    pub(crate) request_id: u64,
    pub(crate) texture: TextureAssetHandle,
    pub(crate) source_width: u32,
    pub(crate) source_height: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) cpu_build_elapsed: Duration,
    pub(crate) upload_elapsed: Duration,
    pub(crate) buffer: GfxBuffer,
}

/// 一次共享 timeline poll 的 typed completion 集合。
#[derive(Default)]
pub(crate) struct RenderAssetUploadCompletions {
    pub(crate) textures: VecDeque<CompletedTextureUpload>,
    pub(crate) sky_distributions: VecDeque<CompletedSkyDistributionUpload>,
}

/// transfer queue 中一条尚未完成的 typed submission。
///
/// command/staging 资源与最终资源必须共享同一个 timeline lifetime。后续 sky
/// distribution 也复用该记录，而不是另建一套 command pool/semaphore。
enum PendingAssetUpload {
    Texture {
        semaphore_value: u64,
        staging_buffer: GfxBuffer,
        command_buffer: GfxCommandBuffer,
        handle: TextureAssetHandle,
        image: GfxImage,
    },
    SkyDistribution {
        semaphore_value: u64,
        staging_buffer: GfxBuffer,
        command_buffer: GfxCommandBuffer,
        request_id: u64,
        texture: TextureAssetHandle,
        source_width: u32,
        source_height: u32,
        width: u32,
        height: u32,
        cpu_build_elapsed: Duration,
        upload_started_at: Instant,
        buffer: GfxBuffer,
    },
}

impl PendingAssetUpload {
    #[inline]
    fn semaphore_value(&self) -> u64 {
        match self {
            Self::Texture { semaphore_value, .. } => *semaphore_value,
            Self::SkyDistribution { semaphore_value, .. } => *semaphore_value,
        }
    }
}

/// `RenderWorld` 私有的共享异步资产上传队列。
///
/// 该 owner 统一持有 transfer command pool、timeline semaphore 和 FIFO pending
/// records。CPU loader/worker 只生产 owned payload；所有 Vulkan 对象仍只在渲染线程
/// 创建、提交、发布和销毁。
pub(crate) struct GpuAssetUploadQueue {
    command_pool: Option<GfxCommandPool>,
    timeline_semaphore: Option<GfxSemaphore>,
    next_timeline_value: u64,
    /// 已移入完成集合的最大 signal 值；CPU 完成查询不替代 graphics 队列的 semaphore wait。
    completed_timeline_value: u64,
    pending_uploads: VecDeque<PendingAssetUpload>,
    /// 完成后尚未被 consumer 逐项接管的资源；发布异常不会丢失其余结果。
    /// submit 未正常返回时不能再派发或发布；仅关闭等待能确认这笔资源不再被 GPU 使用。
    destroyed: bool,
}

impl GpuAssetUploadQueue {
    /// 创建服务 render-side 资产 copy 的 transfer command pool 与 timeline。
    pub(crate) fn new(device_ctx: GfxDeviceCtx<'_>, queue_ctx: GfxQueueCtx<'_>) -> Self {
        let command_pool = GfxCommandPool::new(
            device_ctx,
            queue_ctx.transfer_queue().queue_family().clone(),
            vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
            "RenderAssetTransferPool",
        );
        let timeline_semaphore = { GfxSemaphore::new_timeline(device_ctx, 0, "RenderAssetTransferTimeline") };

        Self {
            command_pool: Some(command_pool),
            timeline_semaphore: Some(timeline_semaphore),
            next_timeline_value: 1,
            completed_timeline_value: 0,
            pending_uploads: VecDeque::new(),

            destroyed: false,
        }
    }

    /// pending 扩容与 timeline 溢出预检先于 native 分配，不消耗失败准备的 timeline 值。
    fn prepare_submission(&mut self) -> anyhow::Result<u64> {
        anyhow::ensure!(!self.destroyed, "asset upload queue used after shutdown");

        let next_value = self
            .next_timeline_value
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("asset upload timeline exhausted"))?;
        self.pending_uploads.try_reserve(1)?;
        Ok(next_value)
    }

    /// 调用前已预留 pending 容量并完成录制；从进入 submit 起不再尝试未提交资源回滚。
    fn submit_prepared(
        &mut self,
        queue_ctx: GfxQueueCtx<'_>,
        next_value: u64,
        batches: Vec<GfxSubmitInfo>,
        upload: PendingAssetUpload,
    ) {
        self.pending_uploads.push_back(upload);
        self.next_timeline_value = next_value;

        queue_ctx.transfer_queue().submit(batches, None);
    }

    /// 将 CPU texture payload 提交到 transfer queue，但暂不发布 image。
    pub(crate) fn submit_texture(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        handle: TextureAssetHandle,
        data: &TextureBytes,
    ) -> anyhow::Result<()> {
        let _span = tracy_client::span!("GpuAssetUploadQueue::submit_texture");
        let next_value = self.prepare_submission()?;
        let command_pool = self.command_pool.as_ref().expect("GpuAssetUploadQueue command pool missing");
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("GpuAssetUploadQueue timeline missing");
        let target_value = self.next_timeline_value;
        let extent = data.extent();
        let image_info = GfxImageCreateInfo::new_image_2d_info(
            vk::Extent2D {
                width: extent.width,
                height: extent.height,
            },
            data.format(),
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
        );
        let image = GfxImage::new(
            resource_ctx,
            &image_info,
            &vk_mem::AllocationCreateInfo {
                usage: vk_mem::MemoryUsage::AutoPreferDevice,
                ..Default::default()
            },
            "AssetTexture",
        );
        let command_buffer = GfxCommandBuffer::new(device_ctx, command_pool, "AssetTextureUploadCmd");
        let command = &command_buffer;
        command.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "AssetTextureUpload");
        let staging_buffer = image.transfer_data(resource_ctx, command, data.as_bytes());
        command.end();
        let batches = vec![GfxSubmitInfo::new(std::slice::from_ref(command)).signal(
            timeline_semaphore,
            vk::PipelineStageFlags2::ALL_COMMANDS,
            Some(target_value),
        )];

        self.submit_prepared(
            queue_ctx,
            next_value,
            batches,
            PendingAssetUpload::Texture {
                semaphore_value: target_value,
                staging_buffer: staging_buffer,
                command_buffer: command_buffer,
                handle,
                image: image,
            },
        );
        Ok(())
    }

    /// 把 CPU Alias entries 提交为 shader device-address buffer。
    ///
    /// copy 后显式建立 `TRANSFER_WRITE -> SHADER_READ` barrier；但 buffer 仍留在 pending
    /// record 中，timeline 完成前其 device address 不会进入 scene root。
    /// CPU entries 在同步录制期间借用；可恢复预检错误先于 native 分配返回。提交前 GPU owner
    /// 进入 pending，submit 失败按致命路径传播，不宣称能恢复未知的提交状态。
    pub(crate) fn submit_sky_distribution(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        build: &SkyDistributionBuild,
    ) -> anyhow::Result<()> {
        let _span = tracy_client::span!("GpuAssetUploadQueue::submit_sky_distribution");
        let byte_size = std::mem::size_of_val(build.entries.as_slice()) as vk::DeviceSize;
        anyhow::ensure!(byte_size > 0, "sky distribution upload must contain at least one entry");
        let next_value = self.prepare_submission()?;
        let command_pool = self.command_pool.as_ref().expect("GpuAssetUploadQueue command pool missing");
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("GpuAssetUploadQueue timeline missing");
        let target_value = self.next_timeline_value;
        let buffer = GfxBuffer::new(
            resource_ctx,
            byte_size,
            vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            None,
            false,
            "SkyDistribution",
        );
        let mut staging_buffer = GfxBuffer::new_stage_buffer(resource_ctx, byte_size, "SkyDistributionStage");
        staging_buffer.transfer_data_by_mmap(resource_ctx, &build.entries);
        let command_buffer = GfxCommandBuffer::new(device_ctx, command_pool, "SkyDistributionUploadCmd");
        let command = &command_buffer;
        command.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "SkyDistributionUpload");
        command.cmd_copy_buffer(
            &staging_buffer,
            &buffer,
            &[vk::BufferCopy {
                size: byte_size,
                ..Default::default()
            }],
        );
        let barrier = GfxBufferBarrier::new()
            .src_mask(vk::PipelineStageFlags2::TRANSFER, vk::AccessFlags2::TRANSFER_WRITE)
            .dst_mask(vk::PipelineStageFlags2::ALL_COMMANDS, vk::AccessFlags2::SHADER_READ)
            .buffer(buffer.vk_buffer(), 0, byte_size);
        command.buffer_memory_barrier(vk::DependencyFlags::empty(), std::slice::from_ref(&barrier));
        command.end();
        let batches = vec![GfxSubmitInfo::new(std::slice::from_ref(command)).signal(
            timeline_semaphore,
            vk::PipelineStageFlags2::ALL_COMMANDS,
            Some(target_value),
        )];
        let upload_started_at = Instant::now();

        self.submit_prepared(
            queue_ctx,
            next_value,
            batches,
            PendingAssetUpload::SkyDistribution {
                semaphore_value: target_value,
                staging_buffer: staging_buffer,
                command_buffer: command_buffer,
                request_id: build.request_id,
                texture: build.texture,
                source_width: build.source_width,
                source_height: build.source_height,
                width: build.width,
                height: build.height,
                cpu_build_elapsed: build.cpu_build_elapsed,
                upload_started_at,
                buffer: buffer,
            },
        );
        Ok(())
    }

    /// 非阻塞查询共享 timeline；consumer 逐项 pop，未接管结果仍由本 owner 保存。
    pub(crate) fn poll(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
    ) -> RenderAssetUploadCompletions {
        let _span = tracy_client::span!("GpuAssetUploadQueue::poll");

        let device = device_ctx.device();
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("GpuAssetUploadQueue used after shutdown");
        // 查询失败不是“尚未完成”；保持 pending owner 不变，并让上层观察设备错误。
        let current_value = unsafe { device.get_semaphore_counter_value(timeline_semaphore.handle()) }
            .expect("failed to query asset upload timeline");

        self.collect_completed(resource_ctx, device_ctx, current_value, DestroyReason::DeferredCleanup)
    }

    /// prepare 与后续 graphics 消费必须等待已发布候选对应的 signal。
    /// 保留该依赖直到关闭，prepare 中断或连续帧使用旧资源时也不会遗漏；不等待尚未完成的上传。
    pub(crate) fn graphics_wait(&self) -> Option<(vk::Semaphore, u64)> {
        assert!(!self.destroyed, "asset upload queue cannot provide a graphics dependency");
        (self.completed_timeline_value != 0).then(|| {
            (
                self.timeline_semaphore.as_ref().expect("asset upload timeline missing").handle(),
                self.completed_timeline_value,
            )
        })
    }

    /// 完成值已确认；先保存目标资源，再释放 staging，诊断失败延后到本次集合收尾之后。
    fn collect_completed(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        current_value: u64,
        reason: DestroyReason,
    ) -> RenderAssetUploadCompletions {
        let mut completed = RenderAssetUploadCompletions::default();
        let command_pool = self.command_pool.as_ref().expect("GpuAssetUploadQueue command pool missing");
        while self.pending_uploads.front().is_some_and(|upload| current_value >= upload.semaphore_value()) {
            let upload = self.pending_uploads.pop_front().unwrap();
            self.completed_timeline_value = upload.semaphore_value();
            let staging_buffer = match upload {
                PendingAssetUpload::Texture {
                    staging_buffer,
                    command_buffer,
                    handle,
                    image,
                    ..
                } => {
                    command_pool.free_command_buffers(device_ctx, vec![command_buffer]);
                    completed.textures.push_back(CompletedTextureUpload { handle, image });
                    staging_buffer
                }
                PendingAssetUpload::SkyDistribution {
                    staging_buffer,
                    command_buffer,
                    request_id,
                    texture,
                    source_width,
                    source_height,
                    width,
                    height,
                    cpu_build_elapsed,
                    upload_started_at,
                    buffer,
                    ..
                } => {
                    command_pool.free_command_buffers(device_ctx, vec![command_buffer]);
                    completed.sky_distributions.push_back(CompletedSkyDistributionUpload {
                        request_id,
                        texture,
                        source_width,
                        source_height,
                        width,
                        height,
                        cpu_build_elapsed,
                        upload_elapsed: upload_started_at.elapsed(),
                        buffer,
                    });
                    staging_buffer
                }
            };
            staging_buffer.destroy(resource_ctx, reason);
        }

        completed
    }

    /// 等待共享 transfer timeline 并释放所有尚未发布的资源。
    ///
    /// 调用者必须先停止所有 CPU producer，保证 shutdown 开始后不会再出现新 submission。
    /// graphics 对本 timeline 的 wait batches 也必须已完成，才能销毁共享 semaphore。
    pub(crate) fn shutdown(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        if self.destroyed {
            return;
        }

        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("GpuAssetUploadQueue timeline missing");

        let last_value = self.pending_uploads.back().map_or(0, PendingAssetUpload::semaphore_value);
        if last_value != 0 {
            const WAIT_SEMAPHORE_TIMEOUT_NS: u64 = 30 * 1000 * 1000 * 1000;
            timeline_semaphore.wait_timeline(device_ctx, last_value, WAIT_SEMAPHORE_TIMEOUT_NS);
        }

        let mut completed = self.collect_completed(resource_ctx, device_ctx, last_value, DestroyReason::Shutdown);
        // 等待成功才接管释放；timeout 或设备错误不能丢失仍在使用的 semaphore/pool/pending。
        let timeline_semaphore = self.timeline_semaphore.take().expect("GpuAssetUploadQueue timeline missing");
        let mut command_pool = self.command_pool.take().expect("GpuAssetUploadQueue command pool missing");

        while let Some(upload) = completed.textures.pop_front() {
            upload.image.destroy(resource_ctx, DestroyReason::Shutdown);
        }
        while let Some(upload) = completed.sky_distributions.pop_front() {
            upload.buffer.destroy(resource_ctx, DestroyReason::Shutdown);
        }

        timeline_semaphore.destroy(device_ctx);

        command_pool.destroy(device_ctx);
        self.destroyed = true;
    }
}

impl Drop for GpuAssetUploadQueue {
    fn drop(&mut self) {
        debug_assert!(self.destroyed, "GpuAssetUploadQueue dropped without explicit shutdown");
    }
}
