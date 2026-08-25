use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use ash::{vk, vk::Handle};

use crate::{
    foundation::debug_messenger::DebugType, gfx::GfxResourceCtx, impl_derive_buffer, resources::buffer::GfxBuffer,
};

pub struct GfxStageBuffer<T: bytemuck::Pod> {
    inner: GfxBuffer,
    _phantom: PhantomData<T>,
}
impl_derive_buffer!(GfxStageBuffer<T: bytemuck::Pod>, GfxBuffer, inner);
impl<T: bytemuck::Pod> GfxStageBuffer<T> {
    pub fn new(ctx: GfxResourceCtx<'_>, debug_name: impl AsRef<str>) -> Self {
        let inner = GfxBuffer::new(
            ctx,
            size_of::<T>() as vk::DeviceSize,
            vk::BufferUsageFlags::TRANSFER_SRC,
            None,
            true,
            debug_name.as_ref(),
        );
        let buffer = Self {
            inner,
            _phantom: PhantomData,
        };
        ctx.device().set_debug_name(&buffer, debug_name);
        buffer
    }
}
impl<T: bytemuck::Pod> DebugType for GfxStageBuffer<T> {
    fn debug_type_name() -> &'static str {
        "StageBuffer"
    }

    fn vk_handle(&self) -> impl Handle {
        self.vk_buffer()
    }
}
