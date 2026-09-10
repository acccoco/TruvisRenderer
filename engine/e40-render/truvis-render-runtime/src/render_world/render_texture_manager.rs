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
use truvis_world::SceneReadView;

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

/// 渲染侧纹理资产上传与绑定缓存。
///
/// 它是 `TextureHandle -> shader texture binding` 的唯一转换点。加载失败或尚未完成上传时，
/// `TextureResolver` 会返回 fallback 纹理，使材质 GPU 数据始终可被 shader 安全读取。
pub struct RenderTextureManager {
    textures: SecondaryMap<TextureHandle, UploadedAssetTexture>,
    pending_textures: HashSet<TextureHandle>,
    fallback: UploadedAssetTexture,
    current_frame_id: u64,
}

impl RenderTextureManager {
    pub(crate) fn begin_frame(&mut self, current_frame_id: u64) {
        self.current_frame_id = current_frame_id;
    }

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
            pending_textures: HashSet::new(),
            fallback,
            current_frame_id: 0,
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

    /// 对账 CPU texture registry，并提交尚未安装的资源到共享 transfer queue。
    ///
    /// 该阶段不查询 timeline、不发布 bindless；完成结果由 `publish_completed_uploads`
    /// 在共享 queue 统一 poll 后接收。
    pub fn submit_uploads(
        &mut self,
        scene: SceneReadView<'_>,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        upload_queue: &mut RenderAssetUploadQueue,
    ) {
        let _span = tracy_client::span!("RenderTextureManager::submit_uploads");
        for handle in scene.texture_handles() {
            if !self.needs_upload(handle) {
                continue;
            }
            let Some(data) = scene.texture_data(handle) else {
                continue;
            };
            match upload_queue.submit_texture(resource_ctx, device_ctx, queue_ctx, handle, data) {
                Ok(()) => {
                    self.pending_textures.insert(handle);
                }
                Err(err) => {
                    log::error!("Failed to submit texture upload {:?}: {}", handle, err);
                }
            }
        }
    }

    /// 发布共享 timeline 已完成的 image，并返回 shader-visible texture 变化。
    pub fn publish_completed_uploads(
        &mut self,
        completed_uploads: Vec<CompletedTextureUpload>,
        scene: SceneReadView<'_>,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) {
        let _span = tracy_client::span!("RenderTextureManager::publish_completed_uploads");
        for completed in completed_uploads {
            let handle = completed.handle;
            let image = completed.image;
            self.pending_textures.remove(&handle);
            if !scene.contains_texture(handle) {
                image.destroy(resource_ctx, DestroyReason::DeferredCleanup);
                continue;
            }
            if self.textures.contains_key(handle) {
                log::error!("RenderTextureManager: reject duplicate upload for immutable texture {:?}", handle);
                image.destroy(resource_ctx, DestroyReason::DeferredCleanup);
                continue;
            }
            self.install_uploaded_texture(
                device_ctx,
                gfx_resource_manager,
                shader_binding_system,
                handle,
                image,
            );
        }
    }

    /// 移除 scene texture 对应的 shader-visible cache。
    ///
    /// 已提交但尚未完成的上传不能取消；completion 通过 CPU registry 的 generational handle
    /// 检查发现删除，timeline 完成后只销毁 image，不会重新 publish。
    pub fn remove_textures(
        &mut self,
        handles: &[TextureHandle],
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) {
        for &handle in handles {
            self.pending_textures.remove(&handle);
            let Some(texture) = self.textures.remove(handle) else {
                continue;
            };
            shader_binding_system.unregister_srv(texture.view_handle);
            gfx_resource_manager.release_image_deferred(texture.image_handle, self.current_frame_id);
        }
    }

    /// 按 CPU registry 的完整 membership 清理已经删除的 render-side texture。
    ///
    /// 删除事实来自最终 scene 状态，不要求 RenderWorld 可靠消费一条 remove event；
    /// 已提交的异步上传在 completion 时再次检查同一 registry，无需长期保存删除 tombstone。
    pub fn remove_stale_textures(
        &mut self,
        scene: SceneReadView<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) -> bool {
        let live_handles = scene.texture_handles().collect::<HashSet<_>>();
        let stale_handles = self
            .textures
            .keys()
            .filter(|handle| !live_handles.contains(handle))
            .collect::<Vec<_>>();
        if stale_handles.is_empty() {
            return false;
        }

        self.remove_textures(
            &stale_handles,
            gfx_resource_manager,
            shader_binding_system,
        );
        true
    }

    fn install_uploaded_texture(
        &mut self,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
        handle: TextureHandle,
        image: GfxImage,
    ) {
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

    pub(crate) fn needs_upload(&self, handle: TextureHandle) -> bool {
        !self.textures.contains_key(handle) && !self.pending_textures.contains(&handle)
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

    fn texture_revision(&self, handle: TextureHandle) -> u64 {
        // 每个 generational handle 的内容不可变，因此只有 fallback -> ready 一次发布；
        // handle 本身区分删除后创建的新资源，0/1 足以表达这一代资源的 binding stamp。
        u64::from(self.textures.contains_key(handle))
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
