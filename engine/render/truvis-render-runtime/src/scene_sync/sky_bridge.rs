use ash::vk;
use slotmap::Key;

use crate::bindings::bindless_manager::BindlessSrvHandle;
use crate::bindings::shader_binding_system::ShaderBindingSystem;
use crate::resources::gfx_resource_manager::GfxResourceManager;
use truvis_asset::asset_hub::AssetHub;
use truvis_asset::handle::AssetTextureHandle;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_path::TruvisPath;
use truvis_render_foundation::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_shader_binding::gpu;

use crate::scene_sync::environment_binding::EnvironmentSkyBinding;
use crate::scene_sync::texture_resolver::TextureResolver;

#[derive(Clone, Copy, Default)]
struct FallbackSkyTexture {
    image_handle: GfxImageHandle,
    view_handle: GfxImageViewHandle,
    srv_handle: BindlessSrvHandle,
}

/// `SkyBridge::update_sky_binding` 的本帧解析结果。
pub(crate) struct SkyBindingUpdate {
    pub(crate) binding: EnvironmentSkyBinding,
    /// sky 绑定是否在 fallback 与真实贴图之间切换。
    pub(crate) changed: bool,
}

/// 默认 sky 的 runtime 私有桥接层。
///
/// `AssetHub` 只负责 sky 图片的 CPU 异步加载；真实 GPU texture 由 `AssetTextureManager`
/// 管理。`SkyBridge` 保存 sky 的内容 handle 和一个常驻纯色 fallback，保证 shader 侧
/// scene root buffer 始终写入合法 SRV。后续 sky PDF / alias table 也应挂在这一层。
pub(crate) struct SkyBridge {
    sky_texture: AssetTextureHandle,
    fallback: FallbackSkyTexture,
    using_real_sky: bool,
}

impl SkyBridge {
    /// 请求默认 sky 异步加载，并创建立即可用的纯色 fallback sky。
    pub(crate) fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        asset_hub: &mut AssetHub,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) -> Self {
        let _span = tracy_client::span!("SkyBridge::new");
        let sky_texture = asset_hub.load_texture(TruvisPath::resources_path("sky.jpg"));
        let fallback = Self::create_fallback_sky(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            shader_binding_system,
        );

        Self {
            sky_texture,
            fallback,
            using_real_sky: false,
        }
    }

    /// 解析本帧 sky 绑定；真实 sky 未 GPU ready 时返回纯色 fallback。
    ///
    /// 这里故意先检查 `is_texture_ready`，避免 `TextureResolver::resolve_texture` 在未就绪时
    /// 返回材质专用的洋红 fallback。sky 的降级策略由本 bridge 独立定义。
    pub(crate) fn update_sky_binding(&mut self, texture_resolver: &dyn TextureResolver) -> SkyBindingUpdate {
        let real_ready = texture_resolver.is_texture_ready(self.sky_texture);
        let changed = self.using_real_sky != real_ready;
        self.using_real_sky = real_ready;

        if changed {
            if real_ready {
                log::info!("SkyBridge: default sky is GPU ready; switch from fallback sky");
            } else {
                log::warn!("SkyBridge: default sky is not GPU ready; switch to fallback sky");
            }
        }

        let binding = if real_ready {
            let texture = texture_resolver.resolve_texture(self.sky_texture);
            EnvironmentSkyBinding {
                srv_handle: texture.srv_handle,
                sampler: gpu::bindless::ESamplerType_LinearClamp,
            }
        } else {
            self.fallback_binding()
        };

        SkyBindingUpdate { binding, changed }
    }

    /// 释放 fallback sky。真实 sky texture 由 `AssetTextureManager` 释放。
    pub(crate) fn destroy_mut(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        shader_binding_system: &mut ShaderBindingSystem,
        gfx_resource_manager: &mut GfxResourceManager,
    ) {
        if !self.fallback.view_handle.is_null() {
            shader_binding_system.unregister_srv(self.fallback.view_handle);
        }
        if !self.fallback.image_handle.is_null() {
            gfx_resource_manager.release_image_immediate(
                resource_ctx,
                device_ctx,
                self.fallback.image_handle,
                DestroyReason::Shutdown,
            );
        }

        self.fallback = FallbackSkyTexture::default();
    }

    fn fallback_binding(&self) -> EnvironmentSkyBinding {
        EnvironmentSkyBinding {
            srv_handle: self.fallback.srv_handle,
            sampler: gpu::bindless::ESamplerType_LinearClamp,
        }
    }

    fn create_fallback_sky(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        shader_binding_system: &mut ShaderBindingSystem,
    ) -> FallbackSkyTexture {
        // sky fallback 需要视觉中性，避免材质缺失用的洋红色污染环境光。
        // shader 当前会将 sky sample 乘以 8，因此这里保持低亮度灰蓝。
        let pixels: [u8; 4] = [10, 13, 15, 255];
        let image = GfxImage::from_rgba8(resource_ctx, immediate_ctx, 1, 1, &pixels, "FallbackSky");
        let image_format = image.format();

        let image_handle = gfx_resource_manager.register_image(image);
        let view_handle = gfx_resource_manager.get_or_create_image_view(
            device_ctx,
            image_handle,
            GfxImageViewDesc::new_2d(image_format, vk::ImageAspectFlags::COLOR),
            "FallbackSkyView",
        );
        shader_binding_system.register_srv(view_handle);
        let srv_handle = shader_binding_system.get_shader_srv_handle(view_handle);

        FallbackSkyTexture {
            image_handle,
            view_handle,
            srv_handle,
        }
    }
}

impl Drop for SkyBridge {
    fn drop(&mut self) {
        debug_assert!(self.fallback.image_handle.is_null());
        debug_assert!(self.fallback.view_handle.is_null());
    }
}
