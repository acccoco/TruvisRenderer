use std::ops::{Deref, DerefMut};

use ash::{vk, vk::Handle};

use crate::resources::layout::GfxIndexType;
use crate::{foundation::debug_messenger::DebugType, gfx::Gfx, impl_derive_buffer, resources::buffer::GfxBuffer};

/// 顶点类型是 u32
pub struct GfxIndexBuffer<T: GfxIndexType> {
    inner: GfxBuffer,

    /// 索引数量
    index_cnt: usize,

    _phantom: std::marker::PhantomData<T>,
}
impl_derive_buffer!(GfxIndexBuffer<T: GfxIndexType>, GfxBuffer, inner);
// 创建与初始化
impl<T: GfxIndexType> GfxIndexBuffer<T> {
    pub fn new_device_local(index_cnt: usize, debug_name: impl AsRef<str>) -> Self {
        Self::new(index_cnt, false, debug_name)
    }

    pub fn new(index_cnt: usize, mmap: bool, debug_name: impl AsRef<str>) -> Self {
        let size = index_cnt * T::byte_size();
        let buffer = GfxBuffer::new(
            size as vk::DeviceSize,
            vk::BufferUsageFlags::INDEX_BUFFER
                | vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
            None,
            mmap,
            debug_name.as_ref(),
        );

        let buffer = Self {
            inner: buffer,
            index_cnt,
            _phantom: std::marker::PhantomData,
        };
        let gfx_device = Gfx::get().gfx_device();
        gfx_device.set_debug_name(&buffer, debug_name);
        buffer
    }

    /// 创建 index buffer，并向其内写入数据
    #[inline]
    pub fn new_with_data(data: &[u32], debug_name: impl AsRef<str>) -> Self {
        let index_buffer = Self::new_device_local(data.len(), debug_name);
        index_buffer.transfer_data_sync(data);
        index_buffer
    }
}
// 访问器
impl<T: GfxIndexType> GfxIndexBuffer<T> {
    #[inline]
    pub fn index_type() -> vk::IndexType {
        T::VK_INDEX_TYPE
    }

    #[inline]
    pub fn index_cnt(&self) -> usize {
        self.index_cnt
    }
}
impl<T: GfxIndexType> DebugType for GfxIndexBuffer<T> {
    fn debug_type_name() -> &'static str {
        "IndexBuffer"
    }

    fn vk_handle(&self) -> impl Handle {
        self.vk_buffer()
    }
}

pub type GfxIndex32Buffer = GfxIndexBuffer<u32>;
pub type GfxIndex16Buffer = GfxIndexBuffer<u16>;
