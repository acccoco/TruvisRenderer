use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use ash::{vk, vk::Handle};

use crate::resources::layout::GfxVertexLayout;
use crate::{
    foundation::debug_messenger::DebugType, gfx::GfxResourceCtx, impl_derive_buffer, resources::buffer::GfxBuffer,
};

pub struct GfxVertexBuffer<L: GfxVertexLayout> {
    inner: GfxBuffer,
    /// 顶点数量
    vertex_cnt: usize,
    _phantom: PhantomData<L>,
}
impl_derive_buffer!(GfxVertexBuffer<L: GfxVertexLayout>, GfxBuffer, inner);
impl<L: GfxVertexLayout> GfxVertexBuffer<L> {
    pub fn new_device_local(ctx: GfxResourceCtx<'_>, vertex_cnt: usize, debug_name: impl AsRef<str>) -> Self {
        Self::new(ctx, vertex_cnt, false, debug_name)
    }

    pub fn new(ctx: GfxResourceCtx<'_>, vertex_cnt: usize, mmap: bool, debug_name: impl AsRef<str>) -> Self {
        let buffer_size = L::buffer_size(vertex_cnt);
        let buffer = GfxBuffer::new(
            ctx,
            buffer_size as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER
                | vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
            None,
            mmap,
            debug_name.as_ref(),
        );

        let buffer = Self {
            inner: buffer,
            vertex_cnt,
            _phantom: PhantomData,
        };
        ctx.device().set_debug_name(&buffer, debug_name);
        buffer
    }

    #[inline]
    pub fn vertex_cnt(&self) -> usize {
        self.vertex_cnt
    }

    #[inline]
    pub fn pos_address(&self) -> vk::DeviceSize {
        self.device_address() + L::pos_offset(self.vertex_cnt)
    }

    #[inline]
    pub fn normal_address(&self) -> vk::DeviceSize {
        self.device_address() + L::normal_offset(self.vertex_cnt)
    }

    #[inline]
    pub fn tangent_address(&self) -> vk::DeviceSize {
        self.device_address() + L::tangent_offset(self.vertex_cnt)
    }

    #[inline]
    pub fn uv_address(&self) -> vk::DeviceSize {
        self.device_address() + L::uv_offset(self.vertex_cnt)
    }
}
impl<L: GfxVertexLayout> DebugType for GfxVertexBuffer<L> {
    fn debug_type_name() -> &'static str {
        "VertexBuffer"
    }

    fn vk_handle(&self) -> impl Handle {
        self.vk_buffer()
    }
}
