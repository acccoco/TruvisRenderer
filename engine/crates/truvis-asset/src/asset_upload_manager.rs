use std::collections::VecDeque;

use ash::vk;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::commands::command_pool::GfxCommandPool;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::commands::submit_info::GfxSubmitInfo;
use truvis_gfx::gfx::Gfx;
use truvis_gfx::resources::buffer::GfxBuffer;
use truvis_gfx::resources::image::{GfxImage, GfxImageCreateInfo};

use crate::asset_loader::RawAssetData;
use crate::handle::AssetTextureHandle;

struct PendingUpload {
    semaphore_value: u64,
    _staging_buffer: GfxBuffer,
    command_buffer: GfxCommandBuffer,
    handle: AssetTextureHandle,
    image: GfxImage,
}

/// 传输管理器
///
/// 负责管理 Vulkan Transfer Queue 的异步上传任务。
/// 核心机制:
/// 1. 使用 Timeline Semaphore 跟踪上传进度，避免为每个任务创建 Fence。
/// 2. 维护一个 Pending 队列，在 update() 中检查 Semaphore 值来回收资源。
/// 3. 自动处理 Staging Buffer 的创建和销毁。
/// 4. 处理 Image Layout 转换 (Undefined -> TransferDst -> ShaderReadOnly)。
pub struct AssetUploadManager {
    command_pool: GfxCommandPool,
    timeline_semaphore: GfxSemaphore,
    next_timeline_value: u64,

    /// 正在等待完成的上传任务队列，会在 update 中检查状态，并且返回已完成的任务
    pending_uploads: VecDeque<PendingUpload>,
}

impl Default for AssetUploadManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetUploadManager {
    pub fn new() -> Self {
        let gfx = Gfx::get();
        let transfer_queue = gfx.transfer_queue();

        // 1. 创建 Command Pool
        let command_pool = GfxCommandPool::new(
            transfer_queue.queue_family().clone(),
            vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
            "AssetTransferPool",
        );

        // 2. 创建 Timeline Semaphore
        let timeline_semaphore = GfxSemaphore::new_timeline(0, "AssetTransferTimeline");

        Self {
            command_pool,
            timeline_semaphore,
            next_timeline_value: 1,
            pending_uploads: VecDeque::new(),
        }
    }

    // TODO image 的 upload，可以考虑每帧合并多个 upload 任务到同一个 Command Buffer 中提交
    /// 提交纹理上传任务
    ///
    /// 流程:
    /// 1. 创建 HostVisible 的 Staging Buffer 并写入像素数据。
    /// 2. 创建 DeviceLocal 的目标 Image。
    /// 3. 分配并录制 Command Buffer:
    ///    - Barrier: Image Undefined -> TransferDst
    ///    - Copy: Buffer -> Image
    ///    - Barrier: Image TransferDst -> ShaderReadOnly
    /// 4. 提交到 Transfer Queue，并设置 Timeline Semaphore 的 Signal 操作。
    pub fn upload_texture(&mut self, data: RawAssetData) -> anyhow::Result<()> {
        let _span = tracy_client::span!("upload_texture");
        let gfx = Gfx::get();

        // 1. 创建目标 Image
        let image_info = GfxImageCreateInfo::new_image_2d_info(
            vk::Extent2D {
                width: data.extent.width,
                height: data.extent.height,
            },
            data.format,
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
        );

        let image = GfxImage::new(
            &image_info,
            &vk_mem::AllocationCreateInfo {
                usage: vk_mem::MemoryUsage::AutoPreferDevice,
                ..Default::default()
            },
            "AssetTexture",
        );

        // 2. 分配 Command Buffer
        let command_buffer = GfxCommandBuffer::new(&self.command_pool, "AssetUploadCmd");

        // 3. 录制命令
        command_buffer.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "AssetUpload");

        // Image2D::transfer_data 负责创建 Staging Buffer，录制 Copy 命令和 Barriers
        // 返回的 Staging Buffer 需要保持存活直到上传完成
        let staging_buffer = image.transfer_data(&command_buffer, &data.pixels);

        command_buffer.end();

        // 4. 提交命令
        let target_value = self.next_timeline_value;
        self.next_timeline_value += 1;

        let submit_info = GfxSubmitInfo::new(std::slice::from_ref(&command_buffer)).signal(
            &self.timeline_semaphore,
            vk::PipelineStageFlags2::ALL_COMMANDS,
            Some(target_value),
        );

        gfx.transfer_queue().submit(vec![submit_info], None);

        // 5. 记录 Pending Upload
        self.pending_uploads.push_back(PendingUpload {
            semaphore_value: target_value,
            _staging_buffer: staging_buffer,
            command_buffer,
            handle: data.handle,
            image,
        });

        Ok(())
    }

    /// 检查上传任务状态
    ///
    /// 必须每帧调用。
    /// 返回已完成上传的资源列表 (Handle + Image)。
    /// 同时负责回收 Staging Buffer 和 Command Buffer。
    pub fn update(&mut self) -> Vec<(AssetTextureHandle, GfxImage)> {
        let _span = tracy_client::span!("TransferManager::update");
        let gfx = Gfx::get();
        let device = gfx.gfx_device();

        // 查询当前 Timeline Semaphore 的值 (非阻塞)
        let current_value =
            unsafe { device.get_semaphore_counter_value(self.timeline_semaphore.handle()).unwrap_or(0) };

        let mut finished_uploads = Vec::new();

        while let Some(upload) = self.pending_uploads.front() {
            if current_value >= upload.semaphore_value {
                // 上传完成
                let upload = self.pending_uploads.pop_front().unwrap();

                // 释放 Command Buffer
                self.command_pool.free_command_buffers(vec![upload.command_buffer]);

                // Staging Buffer 会在 upload 被 drop 时自动销毁

                finished_uploads.push((upload.handle, upload.image));
            } else {
                // 队列是有序的，如果队头未完成，后续肯定也未完成
                break;
            }
        }

        finished_uploads
    }
}

impl Drop for AssetUploadManager {
    fn drop(&mut self) {
        self.timeline_semaphore.clone().destroy();
        self.command_pool.destroy();
    }
}
