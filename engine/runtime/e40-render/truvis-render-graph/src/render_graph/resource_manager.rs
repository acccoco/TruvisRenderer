use slotmap::{Key, SlotMap};

use super::resource_handle::RgImageHandle;
use crate::render_graph::image_resource::RgImageResource;

/// 资源注册表
///
/// 管理 RenderGraph 中所有声明的资源，提供虚拟句柄到资源信息的映射。
/// 使用 SlotMap 存储资源，提供稳定的句柄和高效的访问。
#[derive(Default)]
pub struct RgResourceManager {
    /// 图像资源表
    images: SlotMap<RgImageHandle, RgImageResource>,
}

// 创建与初始化
impl RgResourceManager {
    /// 创建新的资源注册表
    pub fn new() -> Self {
        Self::default()
    }
}

// 注册
impl RgResourceManager {
    pub fn register_image(&mut self, rg_image_resource: RgImageResource) -> RgImageHandle {
        let image = rg_image_resource.physical_handle().expect("graph only supports imported images");
        assert!(!image.is_null(), "cannot import a null image");
        assert!(
            rg_image_resource.physical_view_handle().is_none_or(|view| !view.is_null()),
            "use None instead of importing a null image view"
        );
        // 当前按 image 跟踪状态，不支持用不同 view 将同一物理图像拆成独立同步域。
        assert!(
            self.images.values().all(|existing| existing.physical_handle() != Some(image)),
            "image {:?} already imported; reuse its graph handle",
            image
        );
        self.images.insert(rg_image_resource)
    }
}

// 访问器 & iter
impl RgResourceManager {
    /// 获取图像资源
    #[inline]
    pub fn get_image(&self, handle: RgImageHandle) -> Option<&RgImageResource> {
        self.images.get(handle)
    }

    /// 获取图像数量
    #[inline]
    pub fn image_count(&self) -> usize {
        self.images.len()
    }

    /// 迭代所有图像资源
    #[inline]
    pub fn iter_images(&self) -> impl Iterator<Item = (RgImageHandle, &RgImageResource)> {
        self.images.iter()
    }
}
