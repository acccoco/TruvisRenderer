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
use truvis_world::guid_new_type::TextureHandle;

use crate::render_world::sky_distribution_builder::SkyDistributionBuild;

/// 已完成的 scene texture image 上传。
///
/// timeline 完成后 image 所有权从共享 queue 移交给 `RenderTextureManager`；在此之前
/// image 不允许注册到 `GfxResourceManager` 或 bindless。
pub(crate) struct CompletedTextureUpload {
    pub(crate) handle: TextureHandle,
    pub(crate) image: GfxImage,
}

/// timeline 已确认完成的 sky distribution buffer。
///
/// `RenderSkyManager` 必须再次校验 request id 与 texture handle，匹配后才可把 device
/// address 发布到 scene root；stale completion 则在这里之后立即安全销毁。
pub(crate) struct CompletedSkyDistributionUpload {
    pub(crate) request_id: u64,
    pub(crate) texture: TextureHandle,
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
    pub(crate) textures: Vec<CompletedTextureUpload>,
    pub(crate) sky_distributions: Vec<CompletedSkyDistributionUpload>,
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
        handle: TextureHandle,
        image: GfxImage,
    },
    SkyDistribution {
        semaphore_value: u64,
        staging_buffer: GfxBuffer,
        command_buffer: GfxCommandBuffer,
        request_id: u64,
        texture: TextureHandle,
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
pub(crate) struct RenderAssetUploadQueue {
    command_pool: Option<GfxCommandPool>,
    timeline_semaphore: Option<GfxSemaphore>,
    next_timeline_value: u64,
    pending_uploads: VecDeque<PendingAssetUpload>,
    destroyed: bool,
}

impl RenderAssetUploadQueue {
    /// 创建服务 render-side 资产 copy 的 transfer command pool 与 timeline。
    pub(crate) fn new(device_ctx: GfxDeviceCtx<'_>, queue_ctx: GfxQueueCtx<'_>) -> Self {
        let command_pool = GfxCommandPool::new(
            device_ctx,
            queue_ctx.transfer_queue().queue_family().clone(),
            vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
            "RenderAssetTransferPool",
        );
        let timeline_semaphore = GfxSemaphore::new_timeline(device_ctx, 0, "RenderAssetTransferTimeline");

        Self {
            command_pool: Some(command_pool),
            timeline_semaphore: Some(timeline_semaphore),
            next_timeline_value: 1,
            pending_uploads: VecDeque::new(),
            destroyed: false,
        }
    }

    /// 将 CPU texture payload 提交到 transfer queue，但暂不发布 image。
    pub(crate) fn submit_texture(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        handle: TextureHandle,
        data: TextureBytes,
    ) -> anyhow::Result<()> {
        let _span = tracy_client::span!("RenderAssetUploadQueue::submit_texture");
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

        let command_pool = self.command_pool.as_ref().expect("RenderAssetUploadQueue used after shutdown");
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("RenderAssetUploadQueue used after shutdown");
        let command_buffer = GfxCommandBuffer::new(device_ctx, command_pool, "AssetTextureUploadCmd");

        command_buffer.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "AssetTextureUpload");
        let staging_buffer = image.transfer_data(resource_ctx, &command_buffer, data.as_bytes());
        command_buffer.end();

        let target_value = self.next_timeline_value;
        self.next_timeline_value += 1;
        let submit_info = GfxSubmitInfo::new(std::slice::from_ref(&command_buffer)).signal(
            timeline_semaphore,
            vk::PipelineStageFlags2::ALL_COMMANDS,
            Some(target_value),
        );
        queue_ctx.transfer_queue().submit(vec![submit_info], None);

        self.pending_uploads.push_back(PendingAssetUpload::Texture {
            semaphore_value: target_value,
            staging_buffer,
            command_buffer,
            handle,
            image,
        });
        Ok(())
    }

    /// 把 CPU Alias entries 提交为 shader device-address buffer。
    ///
    /// copy 后显式建立 `TRANSFER_WRITE -> SHADER_READ` barrier；但 buffer 仍留在 pending
    /// record 中，timeline 完成前其 device address 不会进入 scene root。
    pub(crate) fn submit_sky_distribution(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        build: SkyDistributionBuild,
    ) -> anyhow::Result<()> {
        let _span = tracy_client::span!("RenderAssetUploadQueue::submit_sky_distribution");
        let byte_size = std::mem::size_of_val(build.entries.as_slice()) as vk::DeviceSize;
        anyhow::ensure!(byte_size > 0, "sky distribution upload must contain at least one entry");

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
        let staging_buffer = GfxBuffer::new_stage_buffer(resource_ctx, byte_size, "SkyDistributionStage");
        staging_buffer.transfer_data_by_mmap(resource_ctx, &build.entries);

        let command_pool = self.command_pool.as_ref().expect("RenderAssetUploadQueue used after shutdown");
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("RenderAssetUploadQueue used after shutdown");
        let command_buffer = GfxCommandBuffer::new(device_ctx, command_pool, "SkyDistributionUploadCmd");
        command_buffer.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "SkyDistributionUpload");
        command_buffer.cmd_copy_buffer(
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
        command_buffer.buffer_memory_barrier(vk::DependencyFlags::empty(), std::slice::from_ref(&barrier));
        command_buffer.end();

        let target_value = self.next_timeline_value;
        self.next_timeline_value += 1;
        let submit_info = GfxSubmitInfo::new(std::slice::from_ref(&command_buffer)).signal(
            timeline_semaphore,
            vk::PipelineStageFlags2::ALL_COMMANDS,
            Some(target_value),
        );
        let upload_started_at = Instant::now();
        queue_ctx.transfer_queue().submit(vec![submit_info], None);

        self.pending_uploads.push_back(PendingAssetUpload::SkyDistribution {
            semaphore_value: target_value,
            staging_buffer,
            command_buffer,
            request_id: build.request_id,
            texture: build.texture,
            source_width: build.source_width,
            source_height: build.source_height,
            width: build.width,
            height: build.height,
            cpu_build_elapsed: build.cpu_build_elapsed,
            upload_started_at,
            buffer,
        });
        Ok(())
    }

    /// 非阻塞查询共享 timeline，并按 FIFO 返回所有 typed completions。
    pub(crate) fn poll(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
    ) -> RenderAssetUploadCompletions {
        let _span = tracy_client::span!("RenderAssetUploadQueue::poll");
        let device = device_ctx.device();
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("RenderAssetUploadQueue used after shutdown");
        let command_pool = self.command_pool.as_ref().expect("RenderAssetUploadQueue used after shutdown");
        let current_value = unsafe { device.get_semaphore_counter_value(timeline_semaphore.handle()).unwrap_or(0) };

        let mut completed = RenderAssetUploadCompletions::default();
        while self.pending_uploads.front().is_some_and(|upload| current_value >= upload.semaphore_value()) {
            let upload = self.pending_uploads.pop_front().unwrap();
            match upload {
                PendingAssetUpload::Texture {
                    staging_buffer,
                    command_buffer,
                    handle,
                    image,
                    ..
                } => {
                    command_pool.free_command_buffers(device_ctx, vec![command_buffer]);
                    staging_buffer.destroy(resource_ctx, DestroyReason::DeferredCleanup);
                    completed.textures.push(CompletedTextureUpload { handle, image });
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
                    staging_buffer.destroy(resource_ctx, DestroyReason::DeferredCleanup);
                    completed.sky_distributions.push(CompletedSkyDistributionUpload {
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
                }
            }
        }
        completed
    }

    /// 等待共享 transfer timeline 并释放所有尚未发布的资源。
    ///
    /// 调用者必须先停止所有 CPU producer，保证 shutdown 开始后不会再出现新 submission。
    pub(crate) fn shutdown(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        if self.destroyed {
            return;
        }

        let Some(timeline_semaphore) = self.timeline_semaphore.take() else {
            self.destroyed = true;
            return;
        };
        let mut command_pool = self.command_pool.take().expect("RenderAssetUploadQueue command pool missing");

        if let Some(last_upload) = self.pending_uploads.back() {
            const WAIT_SEMAPHORE_TIMEOUT_NS: u64 = 30 * 1000 * 1000 * 1000;
            timeline_semaphore.wait_timeline(device_ctx, last_upload.semaphore_value(), WAIT_SEMAPHORE_TIMEOUT_NS);
        }

        while let Some(upload) = self.pending_uploads.pop_front() {
            match upload {
                PendingAssetUpload::Texture {
                    staging_buffer,
                    command_buffer,
                    image,
                    ..
                } => {
                    command_pool.free_command_buffers(device_ctx, vec![command_buffer]);
                    image.destroy(resource_ctx, DestroyReason::Shutdown);
                    staging_buffer.destroy(resource_ctx, DestroyReason::Shutdown);
                }
                PendingAssetUpload::SkyDistribution {
                    staging_buffer,
                    command_buffer,
                    buffer,
                    ..
                } => {
                    command_pool.free_command_buffers(device_ctx, vec![command_buffer]);
                    buffer.destroy(resource_ctx, DestroyReason::Shutdown);
                    staging_buffer.destroy(resource_ctx, DestroyReason::Shutdown);
                }
            }
        }

        timeline_semaphore.destroy(device_ctx);
        command_pool.destroy(device_ctx);
        self.destroyed = true;
    }
}

impl Drop for RenderAssetUploadQueue {
    fn drop(&mut self) {
        debug_assert!(self.destroyed, "RenderAssetUploadQueue dropped without explicit shutdown");
    }
}
