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
        let inner = GfxBuffer::new_stage_buffer(ctx, size_of::<T>() as vk::DeviceSize, debug_name);
        Self {
            inner,
            _phantom: PhantomData,
        }
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
