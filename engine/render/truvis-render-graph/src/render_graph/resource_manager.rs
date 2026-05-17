use slotmap::SlotMap;

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
