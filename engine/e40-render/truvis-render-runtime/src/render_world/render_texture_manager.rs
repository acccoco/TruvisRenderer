use std::collections::HashSet;

use ash::vk;
use slotmap::SecondaryMap;

use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxQueueCtx, GfxResourceCtx};
use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_foundation::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_shader_binding::gpu;
use truvis_world::guid_new_type::TextureHandle;
use truvis_world::{FailedTextureLoad, PendingTextureUpload};

use crate::bindings::bindless_manager::BindlessSrvHandle;
use crate::bindings::shader_binding_system::ShaderBindingSystem;
use crate::render_world::render_asset_upload_queue::{CompletedTextureUpload, RenderAssetUploadQueue};
use crate::render_world::texture_resolver::{TextureBinding, TextureResolver};
use crate::resources::gfx_resource_manager::GfxResourceManager;

/// shader 可见的纹理绑定缓存。
///
/// `image_handle`/`view_handle` 归 `GfxResourceManager` 管理，`srv_handle` 是 bindless 表中的稳定引用。
/// 材质解析只需要后两者，不直接接触上传队列或 loader owner。
#[derive(Clone, Copy)]
pub struct UploadedAssetTexture {
    /// 注册到 `GfxResourceManager` 的 image owner handle。
    pub image_handle: GfxImageHandle,
    /// shader SRV 使用的 image view handle。
    pub view_handle: GfxImageViewHandle,
    /// bindless 表中的稳定 SRV 引用。
    pub srv_handle: BindlessSrvHandle,
    /// 材质写入 GPU buffer 时使用的 sampler 类型。
    pub sampler: gpu::engine::bindless::ESamplerType,
}

/// texture 上传阶段对 dirty routing 暴露的结构化结果。
#[derive(Default)]
pub(crate) struct RenderTextureUpdateResult {
    /// 本帧从 uploading/fallback 状态发布为 shader-visible ready 的 scene texture。
    pub(crate) ready_changed_textures: Vec<TextureHandle>,
}

/// 渲染侧纹理资产上传与绑定缓存。
///
/// 它是 `TextureHandle -> shader texture binding` 的唯一转换点。加载失败或尚未完成上传时，
/// `TextureResolver` 会返回 fallback 纹理，使材质 GPU 数据始终可被 shader 安全读取。
pub struct RenderTextureManager {
    textures: SecondaryMap<TextureHandle, UploadedAssetTexture>,
    retired_textures: HashSet<TextureHandle>,
    fallback: UploadedAssetTexture,
}

impl RenderTextureManager {
    /// 创建纹理资产管理器，并注册常驻 fallback texture。
    ///
    /// fallback texture 在真实贴图未加载、加载失败或上传未完成时被 `TextureResolver` 返回，
    /// 因此材质 buffer 永远不会写入无效 SRV。
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) -> Self {
        let _span = tracy_client::span!("RenderTextureManager::new");

        let fallback = {
            let _span = tracy_client::span!("RenderTextureManager::new/fallback_texture");
            Self::create_fallback_texture(
                resource_ctx,
                device_ctx,
                immediate_ctx,
                gfx_resource_manager,
                shader_binding_system,
            )
        };

        Self {
            textures: SecondaryMap::new(),
            retired_textures: HashSet::new(),
            fallback,
        }
    }

    fn create_fallback_texture(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) -> UploadedAssetTexture {
        // fallback 使用醒目的 1x1 洋红色纹理，目的是让缺失/未就绪纹理在画面中容易定位；
        // 它在 manager 生命周期内常驻 bindless，避免材质上传阶段产生空 SRV。
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
        shader_binding_system.register_srv(view_handle);
        let srv_handle = shader_binding_system.get_shader_srv_handle(view_handle);

        UploadedAssetTexture {
            image_handle,
            view_handle,
            srv_handle,
            sampler: gpu::engine::bindless::ESamplerType_LinearRepeat,
        }
    }

    /// 消费 texture typed payload，并提交到 `RenderWorld` 共享 transfer queue。
    ///
    /// 该阶段不查询 timeline、不发布 bindless；完成结果由 `publish_completed_uploads`
    /// 在共享 queue 统一 poll 后接收。
    pub fn submit_uploads(
        &mut self,
        pending_uploads: Vec<PendingTextureUpload>,
        failed_textures: Vec<FailedTextureLoad>,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        upload_queue: &mut RenderAssetUploadQueue,
    ) {
        let _span = tracy_client::span!("RenderTextureManager::submit_uploads");
        for upload in pending_uploads {
            if self.retired_textures.contains(&upload.handle) {
                continue;
            }
            if let Err(err) =
                upload_queue.submit_texture(resource_ctx, device_ctx, queue_ctx, upload.handle, upload.data)
            {
                log::error!("Failed to submit texture upload {:?}: {}", upload.handle, err);
            }
        }
        for failed in failed_textures {
            log::error!("Texture load failed {:?}: {}", failed.handle, failed.error);
        }
    }

    /// 发布共享 timeline 已完成的 image，并返回 shader-visible texture 变化。
    pub fn publish_completed_uploads(
        &mut self,
        completed_uploads: Vec<CompletedTextureUpload>,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) -> RenderTextureUpdateResult {
        let _span = tracy_client::span!("RenderTextureManager::publish_completed_uploads");
        let mut result = RenderTextureUpdateResult::default();
        for completed in completed_uploads {
            let handle = completed.handle;
            let image = completed.image;
            if self.retired_textures.remove(&handle) {
                image.destroy(resource_ctx, DestroyReason::DeferredCleanup);
                continue;
            }
            self.replace_uploaded_texture(
                resource_ctx,
                device_ctx,
                gfx_resource_manager,
                shader_binding_system,
                handle,
                image,
            );
            result.ready_changed_textures.push(handle);
        }

        result
    }

    /// 移除 scene texture 对应的 shader-visible cache。
    ///
    /// 已提交但尚未完成的上传不能取消；这里把 handle 记录为 retired，timeline 完成后只销毁
    /// image，不会重新 publish 到 bindless resolver。
    pub fn remove_textures(
        &mut self,
        handles: &[TextureHandle],
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) {
        for &handle in handles {
            self.retired_textures.insert(handle);
            let Some(texture) = self.textures.remove(handle) else {
                continue;
            };
            shader_binding_system.unregister_srv(texture.view_handle);
            gfx_resource_manager.release_image_immediate(
                resource_ctx,
                device_ctx,
                texture.image_handle,
                DestroyReason::ImmediateRelease,
            );
        }
    }

    fn replace_uploaded_texture(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
        handle: TextureHandle,
        image: GfxImage,
    ) {
        // 真实 texture 完成上传后才进入这个函数；从这里开始 resolver 会把 scene texture handle
        // 解析为真实 SRV，material manager 在后续 dirty 检测中把 fallback 替换出去。
        if let Some(old_texture) = self.textures.remove(handle) {
            // 同一个 scene texture handle 重新加载时，旧 view 必须先退出 bindless，再释放 manager-owned image。
            // 这里立即释放的前提是 begin_frame 已经等待过 FIF timeline，旧资源不会再被在flight-frame引用。
            shader_binding_system.unregister_srv(old_texture.view_handle);
            gfx_resource_manager.release_image_immediate(
                resource_ctx,
                device_ctx,
                old_texture.image_handle,
                DestroyReason::ImmediateRelease,
            );
        }

        let image_format = image.format();
        // 只有上传完成的 image 才进入全局资源管理器和 bindless 表。
        // 从这一步开始，材质桥接层解析同一个 TextureHandle 时会拿到真实 SRV。
        let image_handle = gfx_resource_manager.register_image(image);
        let view_handle = gfx_resource_manager.get_or_create_image_view(
            device_ctx,
            image_handle,
            GfxImageViewDesc::new_2d(image_format, vk::ImageAspectFlags::COLOR),
            "AssetTextureView",
        );
        shader_binding_system.register_srv(view_handle);
        let srv_handle = shader_binding_system.get_shader_srv_handle(view_handle);

        let texture = UploadedAssetTexture {
            image_handle,
            view_handle,
            srv_handle,
            sampler: gpu::engine::bindless::ESamplerType_LinearRepeat,
        };
        self.textures.insert(handle, texture);
    }

    /// 关闭上传队列并释放所有已注册纹理。
    ///
    /// shutdown 会等待 pending transfer 完成，因为 staging/image/command buffer 可能仍被 queue 引用。
    /// 调用后 manager 不应再被 `TextureResolver` 使用。
    pub fn destroy(
        mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) {
        for (_, texture) in self.textures.drain() {
            shader_binding_system.unregister_srv(texture.view_handle);
            gfx_resource_manager.release_image_immediate(
                resource_ctx,
                device_ctx,
                texture.image_handle,
                DestroyReason::Shutdown,
            );
        }

        shader_binding_system.unregister_srv(self.fallback.view_handle);
        gfx_resource_manager.release_image_immediate(
            resource_ctx,
            device_ctx,
            self.fallback.image_handle,
            DestroyReason::Shutdown,
        );
    }
}

impl TextureResolver for RenderTextureManager {
    fn is_texture_ready(&self, handle: TextureHandle) -> bool {
        self.textures.contains_key(handle)
    }

    fn resolve_texture(&self, handle: TextureHandle) -> TextureBinding {
        // 解析接口永远返回可写入 material buffer 的 binding。未 ready 或失败的 texture
        // 走 fallback，避免 shader 读取空 bindless 句柄。
        let texture = self.textures.get(handle).unwrap_or(&self.fallback);
        TextureBinding {
            srv_handle: texture.srv_handle,
            sampler: texture.sampler,
        }
    }
}
