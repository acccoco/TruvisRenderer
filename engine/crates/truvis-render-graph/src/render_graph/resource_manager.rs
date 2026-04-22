use slotmap::SlotMap;

use super::resource_handle::{RgBufferHandle, RgImageHandle};
use crate::render_graph::RgImageResource;
use crate::render_graph::buffer_resource::RgBufferResource;

/// 资源注册表
///
/// 管理 RenderGraph 中所有声明的资源，提供虚拟句柄到资源信息的映射。
/// 使用 SlotMap 存储资源，提供稳定的句柄和高效的访问。
#[derive(Default)]
pub struct RgResourceManager {
    /// 图像资源表
    images: SlotMap<RgImageHandle, RgImageResource>,
    /// 缓冲区资源表
    buffers: SlotMap<RgBufferHandle, RgBufferResource>,
}

// new & init
impl RgResourceManager {
    /// 创建新的资源注册表
    pub fn new() -> Self {
        Self::default()
    }
}

// register
impl RgResourceManager {
    pub fn register_image(&mut self, rg_image_resource: RgImageResource) -> RgImageHandle {
        self.images.insert(rg_image_resource)
    }
    pub fn register_buffer(&mut self, rg_buffer_resource: RgBufferResource) -> RgBufferHandle {
        self.buffers.insert(rg_buffer_resource)
    }
}

// getter & iter
impl RgResourceManager {
    /// 获取图像资源
    #[inline]
    pub fn get_image(&self, handle: RgImageHandle) -> Option<&RgImageResource> {
        self.images.get(handle)
    }

    /// 获取可变图像资源
    #[inline]
    pub fn get_image_mut(&mut self, handle: RgImageHandle) -> Option<&mut RgImageResource> {
        self.images.get_mut(handle)
    }

    /// 获取缓冲区资源
    #[inline]
    pub fn get_buffer(&self, handle: RgBufferHandle) -> Option<&RgBufferResource> {
        self.buffers.get(handle)
    }

    /// 获取可变缓冲区资源
    #[inline]
    pub fn get_buffer_mut(&mut self, handle: RgBufferHandle) -> Option<&mut RgBufferResource> {
        self.buffers.get_mut(handle)
    }

    /// 获取图像数量
    #[inline]
    pub fn image_count(&self) -> usize {
        self.images.len()
    }

    /// 获取缓冲区数量
    #[inline]
    pub fn buffer_count(&self) -> usize {
        self.buffers.len()
    }

    /// 迭代所有图像资源
    #[inline]
    pub fn iter_images(&self) -> impl Iterator<Item = (RgImageHandle, &RgImageResource)> {
        self.images.iter()
    }

    /// 迭代所有缓冲区资源
    #[inline]
    pub fn iter_buffers(&self) -> impl Iterator<Item = (RgBufferHandle, &RgBufferResource)> {
        self.buffers.iter()
    }
}
