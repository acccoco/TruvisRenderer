use ash::vk;
use ash::vk::Handle;

use crate::{foundation::debug_messenger::DebugType, gfx::Gfx};

pub struct GfxImageView {
    handle: vk::ImageView,

    desc: GfxImageViewDesc,

    name: String,
}
impl DebugType for GfxImageView {
    fn debug_type_name() -> &'static str {
        "GfxImage2DView"
    }

    fn vk_handle(&self) -> impl vk::Handle {
        self.handle
    }
}
// new & init
impl GfxImageView {
    pub fn new(image: vk::Image, view_desc: GfxImageViewDesc, name: impl AsRef<str>) -> Self {
        let gfx_device = Gfx::get().gfx_device();

        let info = vk::ImageViewCreateInfo {
            image,
            view_type: view_desc.view_type,
            format: view_desc.format,
            subresource_range: vk::ImageSubresourceRange {
                aspect_mask: view_desc.aspect_mask,
                base_mip_level: view_desc.mip.0 as u32,
                level_count: view_desc.mip.1 as u32,
                base_array_layer: 0,
                layer_count: 1,
            },
            ..Default::default()
        };

        let handle = unsafe { gfx_device.create_image_view(&info, None).expect("Failed to create GfxImageView") };
        let image_view = Self {
            handle,

            desc: view_desc,

            name: name.as_ref().to_string(),
        };
        gfx_device.set_debug_name(&image_view, &name);
        image_view
    }
}
// destory
impl GfxImageView {
    pub fn destroy(mut self) {
        self.destroy_mut();
    }
    pub fn destroy_mut(&mut self) {
        unsafe {
            let gfx_device = Gfx::get().gfx_device();
            gfx_device.destroy_image_view(self.handle, None);
        }
        self.handle = vk::ImageView::null();
    }
}
impl Drop for GfxImageView {
    fn drop(&mut self) {
        debug_assert!(self.handle.is_null());
    }
}
// getters
impl GfxImageView {
    /// getter
    #[inline]
    pub fn handle(&self) -> vk::ImageView {
        self.handle
    }
    #[inline]
    pub fn desc(&self) -> &GfxImageViewDesc {
        &self.desc
    }
}
impl std::fmt::Display for GfxImageView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image2DView({}, {:?})", self.name, self.handle)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GfxImageViewDesc {
    /// format 可以基于 vk::Image 重解释
    pub(crate) format: vk::Format,
    /// view type 可以基于 vk::Image 重解释
    pub(crate) view_type: vk::ImageViewType,
    /// aspect 可以基于 vk::Image 重解释
    pub(crate) aspect_mask: vk::ImageAspectFlags,
    /// base mip level 和 mip level count
    pub(crate) mip: (u8, u8),
    /// base layer 和 layer count
    pub(crate) layer: (u8, u8),
}
impl GfxImageViewDesc {
    pub fn new_2d(format: vk::Format, aspect: vk::ImageAspectFlags) -> Self {
        Self {
            format,
            view_type: vk::ImageViewType::TYPE_2D,
            aspect_mask: aspect,
            mip: (0, 1),
            layer: (0, 1),
        }
    }

    /// 创建完整的视图描述
    ///
    /// # 参数
    /// - `format`: 图像格式（可重解释）
    /// - `view_type`: 视图类型（2D, 3D, Cube, Array 等）
    /// - `aspect_mask`: 图像 aspect（COLOR, DEPTH, STENCIL）
    /// - `mip_range`: (base_mip_level, level_count)
    /// - `layer_range`: (base_array_layer, layer_count)
    pub fn new(
        format: vk::Format,
        view_type: vk::ImageViewType,
        aspect_mask: vk::ImageAspectFlags,
        mip_range: (u8, u8),
        layer_range: (u8, u8),
    ) -> Self {
        Self {
            format,
            view_type,
            aspect_mask,
            mip: mip_range,
            layer: layer_range,
        }
    }

    /// 获取格式
    #[inline]
    pub fn format(&self) -> vk::Format {
        self.format
    }

    /// 获取视图类型
    #[inline]
    pub fn view_type(&self) -> vk::ImageViewType {
        self.view_type
    }

    /// 获取 aspect mask
    #[inline]
    pub fn aspect_mask(&self) -> vk::ImageAspectFlags {
        self.aspect_mask
    }

    /// 获取 mip 范围 (base, count)
    #[inline]
    pub fn mip_range(&self) -> (u8, u8) {
        self.mip
    }

    /// 获取 layer 范围 (base, count)
    #[inline]
    pub fn layer_range(&self) -> (u8, u8) {
        self.layer
    }
}
