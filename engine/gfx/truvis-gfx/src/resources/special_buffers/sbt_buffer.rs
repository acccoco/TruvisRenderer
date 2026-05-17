use std::ops::{Deref, DerefMut};

use ash::{vk, vk::Handle};

use crate::{
    foundation::debug_messenger::DebugType, gfx::GfxResourceCtx, impl_derive_buffer, resources::buffer::GfxBuffer,
};

pub struct GfxSBTBuffer {
    _inner: GfxBuffer,
}
impl_derive_buffer!(GfxSBTBuffer, GfxBuffer, _inner);
// 初始化与销毁
impl GfxSBTBuffer {
    pub fn new(ctx: GfxResourceCtx<'_>, size: vk::DeviceSize, align: vk::DeviceSize, name: impl AsRef<str>) -> Self {
        let inner = GfxBuffer::new(
            ctx,
            size,
            vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            Some(align),
            true,
            format!("SBTBuffer::{}", name.as_ref()),
        );
        let buffer = Self { _inner: inner };
        ctx.device().set_debug_name(&buffer, name.as_ref());
        buffer
    }
}
impl DebugType for GfxSBTBuffer {
    fn debug_type_name() -> &'static str {
        "SBTBuffer"
    }

    fn vk_handle(&self) -> impl Handle {
        self.vk_buffer()
    }
}
