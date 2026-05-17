use std::collections::VecDeque;

use ash::vk;
use slotmap::SecondaryMap;

use truvis_asset::asset_hub::LoadedAssetEvent;
use truvis_asset::handle::{AssetTextureHandle, LoadedTextureBytes};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::commands::command_pool::GfxCommandPool;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::commands::submit_info::GfxSubmitInfo;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxQueueCtx, GfxResourceCtx};
use truvis_gfx::resources::buffer::GfxBuffer;
use truvis_gfx::resources::image::{GfxImage, GfxImageCreateInfo};
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_interface::bindless_manager::{BindlessManager, BindlessSrvHandle};
use truvis_render_interface::gfx_resource_manager::GfxResourceManager;
use truvis_render_interface::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_shader_binding::gpu;

use crate::material_manager::{TextureBinding, TextureResolver};

struct PendingUpload {
    semaphore_value: u64,
    staging_buffer: GfxBuffer,
    command_buffer: GfxCommandBuffer,
    handle: AssetTextureHandle,
    image: GfxImage,
}

/// 纹理上传队列。
///
/// 只在渲染线程使用，负责把 `AssetHub` 产出的 CPU bytes 提交到 transfer queue。
/// 完成检测不阻塞帧循环，而是通过 queue-local timeline semaphore 在后续 `update` 中回收
/// command/staging 资源，并把已完成 image 交给上层注册到 `GfxResourceManager` 与 bindless。
struct TextureUploadManager {
    command_pool: Option<GfxCommandPool>,
    timeline_semaphore: Option<GfxSemaphore>,
    next_timeline_value: u64,
    pending_uploads: VecDeque<PendingUpload>,
    destroyed: bool,
}

impl TextureUploadManager {
    fn new(device_ctx: GfxDeviceCtx<'_>, queue_ctx: GfxQueueCtx<'_>) -> Self {
        let command_pool = GfxCommandPool::new(
            device_ctx,
            queue_ctx.transfer_queue().queue_family().clone(),
            vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
            "AssetTransferPool",
        );
        let timeline_semaphore = GfxSemaphore::new_timeline(device_ctx, 0, "AssetTransferTimeline");

        Self {
            command_pool: Some(command_pool),
            timeline_semaphore: Some(timeline_semaphore),
            next_timeline_value: 1,
            pending_uploads: VecDeque::new(),
            destroyed: false,
        }
    }

    fn upload_texture(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        handle: AssetTextureHandle,
        data: LoadedTextureBytes,
    ) -> anyhow::Result<()> {
        let _span = tracy_client::span!("TextureUploadManager::upload_texture");

        // image 先保持在 uploader 私有状态中，只有 timeline 表明确认 copy 完成后，
        // 才注册为 shader 可见资源，避免 bindless 句柄指向仍在上传中的 texture。
        let image_info = GfxImageCreateInfo::new_image_2d_info(
            vk::Extent2D {
                width: data.extent.width,
                height: data.extent.height,
            },
            data.format,
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

        let command_pool = self.command_pool.as_ref().expect("TextureUploadManager used after shutdown");
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("TextureUploadManager used after shutdown");
        let command_buffer = GfxCommandBuffer::new(device_ctx, command_pool, "AssetUploadCmd");

        command_buffer.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "AssetUpload");
        let staging_buffer = image.transfer_data(resource_ctx, &command_buffer, &data.pixels);
        command_buffer.end();

        let target_value = self.next_timeline_value;
        self.next_timeline_value += 1;

        // 每次上传使用单调递增的 timeline value。update 阶段只查询 semaphore counter，
        // 不等待 GPU，因此纹理 ready 状态天然是异步推进的。
        let submit_info = GfxSubmitInfo::new(std::slice::from_ref(&command_buffer)).signal(
            timeline_semaphore,
            vk::PipelineStageFlags2::ALL_COMMANDS,
            Some(target_value),
        );
        queue_ctx.transfer_queue().submit(vec![submit_info], None);

        self.pending_uploads.push_back(PendingUpload {
            semaphore_value: target_value,
            staging_buffer,
            command_buffer,
            handle,
            image,
        });

        Ok(())
    }

    fn update(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
    ) -> Vec<(AssetTextureHandle, GfxImage)> {
        let _span = tracy_client::span!("TextureUploadManager::update");
        let device = device_ctx.device();
        let timeline_semaphore = self.timeline_semaphore.as_ref().expect("TextureUploadManager used after shutdown");
        let command_pool = self.command_pool.as_ref().expect("TextureUploadManager used after shutdown");
        let current_value = unsafe { device.get_semaphore_counter_value(timeline_semaphore.handle()).unwrap_or(0) };

        let mut finished_uploads = Vec::new();
        while let Some(upload) = self.pending_uploads.front() {
            if current_value < upload.semaphore_value {
                break;
            }

            // GPU 已经完成 copy，staging buffer 与一次性 command buffer 可以立即释放；
            // device-local image 的所有权转交给 AssetTextureUploader 注册 view/bindless。
            let upload = self.pending_uploads.pop_front().unwrap();
            command_pool.free_command_buffers(device_ctx, vec![upload.command_buffer]);
            upload.staging_buffer.destroy(resource_ctx, DestroyReason::DeferredCleanup);
            finished_uploads.push((upload.handle, upload.image));
        }

        finished_uploads
    }

    fn shutdown(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        if self.destroyed {
            return;
        }

        let Some(timeline_semaphore) = self.timeline_semaphore.take() else {
            self.destroyed = true;
            return;
        };
        let mut command_pool = self.command_pool.take().expect("TextureUploadManager command pool missing");

        if let Some(last_upload) = self.pending_uploads.back() {
            const WAIT_SEMAPHORE_TIMEOUT_NS: u64 = 30 * 1000 * 1000 * 1000;
            timeline_semaphore.wait_timeline(device_ctx, last_upload.semaphore_value, WAIT_SEMAPHORE_TIMEOUT_NS);
        }

        // shutdown 必须等待所有 pending 上传完成，因为这些 image/staging/command 仍可能被 transfer queue 引用。
        while let Some(upload) = self.pending_uploads.pop_front() {
            command_pool.free_command_buffers(device_ctx, vec![upload.command_buffer]);
            upload.image.destroy(resource_ctx, DestroyReason::Shutdown);
            upload.staging_buffer.destroy(resource_ctx, DestroyReason::Shutdown);
        }

        timeline_semaphore.destroy(device_ctx);
        command_pool.destroy(device_ctx);
        self.destroyed = true;
    }
}

impl Drop for TextureUploadManager {
    fn drop(&mut self) {
        debug_assert!(self.destroyed, "TextureUploadManager dropped without explicit shutdown");
    }
}

/// shader 可见的纹理绑定缓存。
///
/// `image_handle`/`view_handle` 归 `GfxResourceManager` 管理，`srv_handle` 是 bindless 表中的稳定引用。
/// 材质解析只需要后两者，不直接接触上传队列或 `AssetHub`。
#[derive(Clone, Copy)]
pub struct UploadedAssetTexture {
    pub image_handle: GfxImageHandle,
    pub view_handle: GfxImageViewHandle,
    pub srv_handle: BindlessSrvHandle,
    pub sampler: gpu::ESamplerType,
}

/// 渲染侧纹理资产上传与绑定缓存。
///
/// 它是 `AssetTextureHandle -> shader texture binding` 的唯一转换点。加载失败或尚未完成上传时，
/// `TextureResolver` 会返回 fallback 纹理，使材质 GPU 数据始终可被 shader 安全读取。
pub struct AssetTextureUploader {
    textures: SecondaryMap<AssetTextureHandle, UploadedAssetTexture>,
    uploader: TextureUploadManager,
    fallback: UploadedAssetTexture,
}

impl AssetTextureUploader {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        bindless_manager: &mut BindlessManager,
    ) -> Self {
        let _span = tracy_client::span!("AssetTextureUploader::new");

        let fallback = {
            let _span = tracy_client::span!("AssetTextureUploader::new/fallback_texture");
            Self::create_fallback_texture(
                resource_ctx,
                device_ctx,
                immediate_ctx,
                gfx_resource_manager,
                bindless_manager,
            )
        };

        let uploader = {
            let _span = tracy_client::span!("AssetTextureUploader::new/upload_manager");
            TextureUploadManager::new(device_ctx, queue_ctx)
        };

        Self {
            textures: SecondaryMap::new(),
            uploader,
            fallback,
        }
    }

    fn create_fallback_texture(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        bindless_manager: &mut BindlessManager,
    ) -> UploadedAssetTexture {
        // fallback 使用醒目的 1x1 洋红色纹理，目的是让缺失/未就绪纹理在画面中容易定位；
        // 它在 uploader 生命周期内常驻 bindless，避免材质上传阶段产生空 SRV。
        let pixels: [u8; 4] = [255, 0, 255, 255];
        let image = GfxImage::from_rgba8(resource_ctx, immediate_ctx, 1, 1, &pixels, "FallbackTexture");
        let image_format = image.format();

        let image_handle = gfx_resource_manager.register_image(image);
        let view_handle = gfx_resource_manager.get_or_create_image_view(
            device_ctx,
            image_handle,
            GfxImageViewDesc::new_2d(image_format, vk::ImageAspectFlags::COLOR),
            "FallbackTextureView",
        );
        bindless_manager.register_srv(view_handle);
        let srv_handle = bindless_manager.get_shader_srv_handle(view_handle);

        UploadedAssetTexture {
            image_handle,
            view_handle,
            srv_handle,
            sampler: gpu::ESamplerType_LinearRepeat,
        }
    }

    pub fn update(
        &mut self,
        events: Vec<LoadedAssetEvent>,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        bindless_manager: &mut BindlessManager,
    ) {
        let _span = tracy_client::span!("AssetTextureUploader::update");

        for event in events {
            match event {
                LoadedAssetEvent::TextureLoaded { handle, data } => {
                    if let Err(err) = self.uploader.upload_texture(resource_ctx, device_ctx, queue_ctx, handle, data) {
                        log::error!("Failed to submit texture upload {:?}: {}", handle, err);
                    }
                }
                LoadedAssetEvent::TextureFailed { handle, error } => {
                    log::error!("Texture load failed {:?}: {}", handle, error);
                }
                LoadedAssetEvent::MeshLoaded { .. }
                | LoadedAssetEvent::SceneLoaded { .. }
                | LoadedAssetEvent::SceneFailed { .. } => {}
            }
        }

        for (handle, image) in self.uploader.update(resource_ctx, device_ctx) {
            self.replace_uploaded_texture(
                resource_ctx,
                device_ctx,
                gfx_resource_manager,
                bindless_manager,
                handle,
                image,
            );
        }
    }

    fn replace_uploaded_texture(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        bindless_manager: &mut BindlessManager,
        handle: AssetTextureHandle,
        image: GfxImage,
    ) {
        if let Some(old_texture) = self.textures.remove(handle) {
            // 同一个 asset handle 重新加载时，旧 view 必须先退出 bindless，再释放 manager-owned image。
            // 这里立即释放的前提是 begin_frame 已经等待过 FIF timeline，旧资源不会再被在飞帧引用。
            bindless_manager.unregister_srv(old_texture.view_handle);
            gfx_resource_manager.release_image_immediate(
                resource_ctx,
                device_ctx,
                old_texture.image_handle,
                DestroyReason::ImmediateRelease,
            );
        }

        let image_format = image.format();
        // 只有上传完成的 image 才进入全局资源管理器和 bindless 表。
        // 从这一步开始，材质桥接层解析同一个 AssetTextureHandle 时会拿到真实 SRV。
        let image_handle = gfx_resource_manager.register_image(image);
        let view_handle = gfx_resource_manager.get_or_create_image_view(
            device_ctx,
            image_handle,
            GfxImageViewDesc::new_2d(image_format, vk::ImageAspectFlags::COLOR),
            "AssetTextureView",
        );
        bindless_manager.register_srv(view_handle);
        let srv_handle = bindless_manager.get_shader_srv_handle(view_handle);

        let texture = UploadedAssetTexture {
            image_handle,
            view_handle,
            srv_handle,
            sampler: gpu::ESamplerType_LinearRepeat,
        };
        self.textures.insert(handle, texture);
    }

    pub fn destroy(
        mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        bindless_manager: &mut BindlessManager,
    ) {
        self.uploader.shutdown(resource_ctx, device_ctx);

        for (_, texture) in self.textures.drain() {
            bindless_manager.unregister_srv(texture.view_handle);
            gfx_resource_manager.release_image_immediate(
                resource_ctx,
                device_ctx,
                texture.image_handle,
                DestroyReason::Shutdown,
            );
        }

        bindless_manager.unregister_srv(self.fallback.view_handle);
        gfx_resource_manager.release_image_immediate(
            resource_ctx,
            device_ctx,
            self.fallback.image_handle,
            DestroyReason::Shutdown,
        );
    }
}

impl TextureResolver for AssetTextureUploader {
    fn is_texture_ready(&self, handle: AssetTextureHandle) -> bool {
        self.textures.contains_key(handle)
    }

    fn resolve_texture(&self, handle: AssetTextureHandle) -> TextureBinding {
        let texture = self.textures.get(handle).unwrap_or(&self.fallback);
        TextureBinding {
            srv_handle: texture.srv_handle,
            sampler: texture.sampler,
        }
    }
}
