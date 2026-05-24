use std::path::Path;

use ash::vk;
use slotmap::Key;

use crate::environment_binding::EnvironmentTextureBinding;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_path::TruvisPath;
use truvis_render_foundation::bindless_manager::BindlessManager;
use truvis_render_foundation::gfx_resource_manager::GfxResourceManager;
use truvis_render_foundation::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_shader_binding::gpu;

/// runtime 默认辅助环境贴图资源。
///
/// 这里仅持有启动期必须立即可用的小贴图，例如 UV checker。默认 sky 已经移到
/// `SkyBridge`，通过 AssetHub 与纹理管理器异步加载。
pub(super) struct DefaultEnvironment {
    uv_checker_texture: (GfxImageHandle, GfxImageViewHandle),
}

impl DefaultEnvironment {
    /// 加载默认辅助贴图、注册 image view，并写入 bindless SRV 表。
    ///
    /// UV checker 在 runtime 生命周期内常驻；动态 scene 上传只读取它的 bindless handle，
    /// 不负责从文件系统加载辅助资源。
    pub(super) fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        bindless_manager: &mut BindlessManager,
    ) -> Self {
        let uv_checker_path = TruvisPath::resources_path("uv_checker.png");

        let uv_checker_image = {
            let _span = tracy_client::span!("DefaultEnvironment::new/load_uv_checker");
            Self::load_image(resource_ctx, immediate_ctx, &uv_checker_path)
        };
        let uv_checker_image_format = uv_checker_image.format();

        let (uv_checker_image_handle, uv_checker_view_handle) = {
            let _span = tracy_client::span!("DefaultEnvironment::new/register_uv_checker_view");
            let uv_checker_image_handle = gfx_resource_manager.register_image(uv_checker_image);
            let uv_checker_view_handle = gfx_resource_manager.get_or_create_image_view(
                device_ctx,
                uv_checker_image_handle,
                GfxImageViewDesc::new_2d(uv_checker_image_format, vk::ImageAspectFlags::COLOR),
                uv_checker_path.to_str().unwrap(),
            );

            (uv_checker_image_handle, uv_checker_view_handle)
        };

        {
            let _span = tracy_client::span!("DefaultEnvironment::new/register_uv_checker_bindless");
            bindless_manager.register_srv(uv_checker_view_handle);
        }

        Self {
            uv_checker_texture: (uv_checker_image_handle, uv_checker_view_handle),
        }
    }

    /// 返回 scene root buffer 写入的 UV checker 绑定。
    pub(super) fn uv_checker_binding(&self, bindless_manager: &BindlessManager) -> EnvironmentTextureBinding {
        EnvironmentTextureBinding {
            srv_handle: bindless_manager.get_shader_srv_handle(self.uv_checker_texture.1),
            sampler: gpu::ESamplerType_LinearClamp,
        }
    }

    /// 注销默认辅助贴图的 bindless SRV，并释放资源管理器中的 image wrapper。
    ///
    /// 字段会置回 null handle，配合 `Drop` 的 debug_assert 检测是否漏掉显式销毁。
    pub(super) fn destroy_mut(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        bindless_manager: &mut BindlessManager,
        gfx_resource_manager: &mut GfxResourceManager,
    ) {
        let (uv_checker_image, uv_checker_view) = self.uv_checker_texture;
        if !uv_checker_view.is_null() {
            bindless_manager.unregister_srv(uv_checker_view);
        }
        if !uv_checker_image.is_null() {
            gfx_resource_manager.release_image_immediate(
                resource_ctx,
                device_ctx,
                uv_checker_image,
                DestroyReason::Shutdown,
            );
        }

        self.uv_checker_texture = (GfxImageHandle::default(), GfxImageViewHandle::default());
    }

    /// 从资源目录读取图片并立即上传成 GPU image。
    ///
    /// 默认辅助贴图属于 runtime 启动必需资源，因此这里保持 fail-fast：缺失文件会在初始化阶段
    /// 直接暴露，而不是在 render 阶段降级为不可见错误。
    fn load_image(resource_ctx: GfxResourceCtx<'_>, immediate_ctx: GfxImmediateCtx<'_>, tex_path: &Path) -> GfxImage {
        let img = image::ImageReader::open(tex_path).unwrap().decode().unwrap().to_rgba8();
        let width = img.width();
        let height = img.height();
        let data = img.as_raw();
        let name = tex_path.to_str().unwrap();

        GfxImage::from_rgba8(resource_ctx, immediate_ctx, width, height, data, name)
    }
}

impl Drop for DefaultEnvironment {
    fn drop(&mut self) {
        debug_assert!(self.uv_checker_texture.0.is_null());
        debug_assert!(self.uv_checker_texture.1.is_null());
    }
}
