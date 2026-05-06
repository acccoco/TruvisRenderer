use std::ops::Deref;
use std::ops::DerefMut;

use ash::vk;

use crate::gfx::GfxResourceCtx;
use crate::impl_derive_buffer;
use crate::resources::buffer::GfxBuffer;

pub struct GfxAccelerationScratchBuffer {
    inner: GfxBuffer,
}
impl_derive_buffer!(GfxAccelerationScratchBuffer, GfxBuffer, inner);
impl GfxAccelerationScratchBuffer {
    pub fn new(ctx: GfxResourceCtx<'_>, size: vk::DeviceSize, name: impl AsRef<str>) -> Self {
        let buffer = GfxBuffer::new(
            ctx,
            size,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            None,
            false,
            name,
        );

        Self { inner: buffer }
    }
}

pub struct GfxAccelerationStructureBuffer {
    inner: GfxBuffer,
}
impl_derive_buffer!(GfxAccelerationStructureBuffer, GfxBuffer, inner);
impl GfxAccelerationStructureBuffer {
    pub fn new(ctx: GfxResourceCtx<'_>, size: vk::DeviceSize, name: impl AsRef<str>) -> Self {
        let buffer = GfxBuffer::new(
            ctx,
            size,
            vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            None,
            false,
            name,
        );

        Self { inner: buffer }
    }
}

pub struct GfxAccelerationInstanceBuffer {
    inner: GfxBuffer,
}
impl_derive_buffer!(GfxAccelerationInstanceBuffer, GfxBuffer, inner);
impl GfxAccelerationInstanceBuffer {
    pub fn new(ctx: GfxResourceCtx<'_>, size: vk::DeviceSize, name: impl AsRef<str>) -> Self {
        let buffer = GfxBuffer::new(
            ctx,
            size,
            vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
                | vk::BufferUsageFlags::TRANSFER_DST,
            None,
            false,
            name,
        );

        Self { inner: buffer }
    }
}
