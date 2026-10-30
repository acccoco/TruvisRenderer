use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use ash::vk;

use crate::{
    foundation::debug_messenger::DebugType,
    gfx::{GfxImmediateCtx, GfxResourceCtx},
    impl_derive_buffer,
    resources::buffer::GfxBuffer,
};

/// buffer 内存放的是结构体或者结构体的数组
pub struct GfxStructuredBuffer<T: Sized> {
    inner: GfxBuffer,
    /// 结构体的数量
    ele_num: usize,
    _phantom: PhantomData<T>,
}
impl_derive_buffer!(GfxStructuredBuffer<T>, GfxBuffer, inner);
impl<T> GfxStructuredBuffer<T> {
    /// 同步创建带初始内容的 SSBO，返回前已等待上传完成。
    pub fn new_ssbo_with_data(
        resource_ctx: GfxResourceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        data: &[T],
        debug_name: impl AsRef<str>,
    ) -> Self
    where
        T: bytemuck::NoUninit,
    {
        let mut buffer = Self::new_ssbo(resource_ctx, data.len(), debug_name);
        buffer.transfer_data_sync(resource_ctx, immediate_ctx, data);
        buffer
    }

    #[inline]
    pub fn new_ssbo(ctx: GfxResourceCtx<'_>, len: usize, debug_name: impl AsRef<str>) -> Self {
        Self::new(
            ctx,
            debug_name,
            len,
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            false,
        )
    }

    #[inline]
    pub fn new_ubo(ctx: GfxResourceCtx<'_>, len: usize, debug_name: impl AsRef<str>) -> Self {
        Self::new(
            ctx,
            debug_name,
            len,
            vk::BufferUsageFlags::UNIFORM_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            false,
        )
    }

    #[inline]
    pub fn new_stage_buffer(ctx: GfxResourceCtx<'_>, len: usize, debug_name: impl AsRef<str>) -> Self {
        Self::new(ctx, debug_name, len, vk::BufferUsageFlags::TRANSFER_SRC, true)
    }

    #[inline]
    pub fn new_readback_buffer(ctx: GfxResourceCtx<'_>, len: usize, debug_name: impl AsRef<str>) -> Self {
        Self::new(ctx, debug_name, len, vk::BufferUsageFlags::TRANSFER_DST, true)
    }

    #[inline]
    pub fn new(
        ctx: GfxResourceCtx<'_>,
        debug_name: impl AsRef<str>,
        len: usize,
        buffer_usage_flags: vk::BufferUsageFlags,
        mapped: bool,
    ) -> Self {
        let size = len.checked_mul(size_of::<T>()).expect("structured buffer size overflow");
        let buffer = GfxBuffer::new(ctx, size as vk::DeviceSize, buffer_usage_flags, None, mapped, debug_name);

        Self {
            inner: buffer,
            ele_num: len,
            _phantom: PhantomData,
        }
    }
}

impl<T: bytemuck::Pod> GfxStructuredBuffer<T> {
    /// 访问前 owner 必须等待 GPU 完成；写后按实际范围 flush，readback 读前 invalidate。
    pub fn mapped_slice(&mut self) -> &mut [T] {
        let mapped_ptr = self.inner.mapped_range_ptr::<T>(self.ele_num);
        unsafe { std::slice::from_raw_parts_mut(mapped_ptr, self.ele_num) }
    }

    pub fn mapped_slice_ref(&self) -> &[T] {
        let mapped_ptr = self.inner.mapped_range_ptr::<T>(self.ele_num);
        unsafe { std::slice::from_raw_parts(mapped_ptr, self.ele_num) }
    }
}
impl<T: bytemuck::Pod> DebugType for GfxStructuredBuffer<T> {
    #[inline]
    fn debug_type_name() -> &'static str {
        "StructuredBuffer"
    }

    #[inline]
    fn vk_handle(&self) -> impl vk::Handle {
        self.vk_buffer()
    }
}
