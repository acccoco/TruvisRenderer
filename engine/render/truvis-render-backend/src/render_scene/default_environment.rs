use std::path::{Path, PathBuf};

use ash::vk;
use slotmap::Key;

use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_path::TruvisPath;
use truvis_render_interface::bindless_manager::{BindlessManager, BindlessSrvHandle};
use truvis_render_interface::gfx_resource_manager::GfxResourceManager;
use truvis_render_interface::handles::{GfxImageHandle, GfxImageViewHandle};

/// backend 默认环境贴图资源。
///
/// 这些贴图是 scene root buffer 的默认 shader 输入，但不属于 `GpuScene` 的动态
/// instance/geometry/TLAS 职责。独立 owner 让 `GpuScene` 只负责消费 bindless handle。
pub(super) struct DefaultEnvironment {
    sky_texture: (GfxImageHandle, GfxImageViewHandle),
    uv_checker_texture: (GfxImageHandle, GfxImageViewHandle),
}

impl DefaultEnvironment {
    /// 加载默认环境贴图、注册 image view，并写入 bindless SRV 表。
    ///
    /// 这些贴图在 backend 生命周期内常驻；动态 scene 上传只读取它们的 bindless handle，
    /// 不负责从文件系统加载默认资源。
    pub(super) fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        bindless_manager: &mut BindlessManager,
    ) -> Self {
        let sky_path = TruvisPath::resources_path_str("sky.jpg");
        let uv_checker_path = TruvisPath::resources_path_str("uv_checker.png");

        let (sky_image, uv_checker_image) = {
            let _span = tracy_client::span!("DefaultEnvironment::new/load_images");
            (
                Self::load_image(resource_ctx, immediate_ctx, &PathBuf::from(&sky_path)),
                Self::load_image(resource_ctx, immediate_ctx, &PathBuf::from(&uv_checker_path)),
            )
        };

        let sky_image_format = sky_image.format();
        let uv_checker_image_format = uv_checker_image.format();

        let (sky_image_handle, sky_view_handle, uv_checker_image_handle, uv_checker_view_handle) = {
            let _span = tracy_client::span!("DefaultEnvironment::new/register_image_views");
            let sky_image_handle = gfx_resource_manager.register_image(sky_image);
            let sky_view_handle = gfx_resource_manager.get_or_create_image_view(
                device_ctx,
                sky_image_handle,
                GfxImageViewDesc::new_2d(sky_image_format, vk::ImageAspectFlags::COLOR),
                &sky_path,
            );

            let uv_checker_image_handle = gfx_resource_manager.register_image(uv_checker_image);
            let uv_checker_view_handle = gfx_resource_manager.get_or_create_image_view(
                device_ctx,
                uv_checker_image_handle,
                GfxImageViewDesc::new_2d(uv_checker_image_format, vk::ImageAspectFlags::COLOR),
                &uv_checker_path,
            );

            (sky_image_handle, sky_view_handle, uv_checker_image_handle, uv_checker_view_handle)
        };

        {
            let _span = tracy_client::span!("DefaultEnvironment::new/register_bindless");
            bindless_manager.register_srv(sky_view_handle);
            bindless_manager.register_srv(uv_checker_view_handle);
        }

        Self {
            sky_texture: (sky_image_handle, sky_view_handle),
            uv_checker_texture: (uv_checker_image_handle, uv_checker_view_handle),
        }
    }

    /// 返回 scene root buffer 写入的 sky texture bindless handle。
    pub(super) fn sky_srv_handle(&self, bindless_manager: &BindlessManager) -> BindlessSrvHandle {
        bindless_manager.get_shader_srv_handle(self.sky_texture.1)
    }

    /// 返回 scene root buffer 写入的 UV checker texture bindless handle。
    pub(super) fn uv_checker_srv_handle(&self, bindless_manager: &BindlessManager) -> BindlessSrvHandle {
        bindless_manager.get_shader_srv_handle(self.uv_checker_texture.1)
    }

    /// 注销默认贴图的 bindless SRV，并释放资源管理器中的 image wrapper。
    ///
    /// 字段会置回 null handle，配合 `Drop` 的 debug_assert 检测是否漏掉显式销毁。
    pub(super) fn destroy_mut(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        bindless_manager: &mut BindlessManager,
        gfx_resource_manager: &mut GfxResourceManager,
    ) {
        let (sky_image, sky_view) = self.sky_texture;
        if !sky_view.is_null() {
            bindless_manager.unregister_srv(sky_view);
        }
        if !sky_image.is_null() {
            gfx_resource_manager.release_image_immediate(resource_ctx, device_ctx, sky_image, DestroyReason::Shutdown);
        }

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

        self.sky_texture = (GfxImageHandle::default(), GfxImageViewHandle::default());
        self.uv_checker_texture = (GfxImageHandle::default(), GfxImageViewHandle::default());
    }

    /// 从资源目录读取图片并立即上传成 GPU image。
    ///
    /// 默认环境贴图属于 backend 启动必需资源，因此这里保持 fail-fast：缺失文件会在初始化阶段
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
        debug_assert!(self.sky_texture.0.is_null());
        debug_assert!(self.sky_texture.1.is_null());
        debug_assert!(self.uv_checker_texture.0.is_null());
        debug_assert!(self.uv_checker_texture.1.is_null());
    }
}
