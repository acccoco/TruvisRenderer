use std::ops::{Deref, DerefMut};

use ash::{vk, vk::Handle};

use crate::{
    foundation::debug_messenger::DebugType,
    gfx::{GfxDeviceCtx, GfxDeviceInfoCtx, GfxResourceCtx},
    impl_derive_buffer,
    resources::buffer::GfxBuffer,
};

pub struct GfxSBTBuffer {
    _inner: GfxBuffer,
    raygen: vk::StridedDeviceAddressRegionKHR,
    miss: vk::StridedDeviceAddressRegionKHR,
    hit: vk::StridedDeviceAddressRegionKHR,
    callable: vk::StridedDeviceAddressRegionKHR,
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
        let buffer = Self {
            _inner: inner,
            raygen: vk::StridedDeviceAddressRegionKHR::default(),
            miss: vk::StridedDeviceAddressRegionKHR::default(),
            hit: vk::StridedDeviceAddressRegionKHR::default(),
            callable: vk::StridedDeviceAddressRegionKHR::default(),
        };
        ctx.device().set_debug_name(&buffer, name.as_ref());
        buffer
    }

    pub fn from_shader_groups(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        device_info_ctx: GfxDeviceInfoCtx<'_>,
        pipeline: vk::Pipeline,
        group_count: u32,
        raygen_group: usize,
        miss_groups: &[usize],
        hit_groups: &[usize],
        callable_groups: &[usize],
        name: impl AsRef<str>,
    ) -> Self {
        let rt_pipeline_props = device_info_ctx.rt_pipeline_props();
        let handle_size = rt_pipeline_props.shader_group_handle_size;
        let handle_stride = handle_size.next_multiple_of(rt_pipeline_props.shader_group_handle_alignment);
        let base_alignment = rt_pipeline_props.shader_group_base_alignment;

        Self::assert_group_index(group_count, raygen_group, "raygen");
        Self::assert_group_indices(group_count, miss_groups, "miss");
        Self::assert_group_indices(group_count, hit_groups, "hit");
        Self::assert_group_indices(group_count, callable_groups, "callable");

        let raygen_size = handle_stride.next_multiple_of(base_alignment);
        let miss_size = Self::region_size(miss_groups.len(), handle_stride, base_alignment);
        let hit_size = Self::region_size(hit_groups.len(), handle_stride, base_alignment);
        let callable_size = Self::region_size(callable_groups.len(), handle_stride, base_alignment);
        let total_size = raygen_size + miss_size + hit_size + callable_size;

        let mut sbt_buffer =
            Self::new(resource_ctx, total_size as vk::DeviceSize, base_alignment as vk::DeviceSize, name);
        let sbt_address = sbt_buffer.device_address();

        sbt_buffer.raygen = Self::region(sbt_address, 0, raygen_size, raygen_size);
        sbt_buffer.miss = Self::region(sbt_address, raygen_size as vk::DeviceSize, handle_stride, miss_size);
        sbt_buffer.hit =
            Self::region(sbt_address, (raygen_size + miss_size) as vk::DeviceSize, handle_stride, hit_size);
        sbt_buffer.callable = Self::region(
            sbt_address,
            (raygen_size + miss_size + hit_size) as vk::DeviceSize,
            handle_stride,
            callable_size,
        );

        let shader_group_handle_data = unsafe {
            device_ctx
                .device()
                .ray_tracing_pipeline()
                .get_ray_tracing_shader_group_handles(pipeline, 0, group_count, (group_count * handle_size) as usize)
                .unwrap()
        };

        // SBT 当前只写 shader group handle，不携带 user data；stride 中多出的对齐空间保持未使用。
        let sbt_host_address = sbt_buffer.mapped_ptr();
        Self::copy_group_handle(&shader_group_handle_data, handle_size as usize, raygen_group, sbt_host_address);

        let miss_host_address = sbt_host_address.wrapping_byte_add(raygen_size as usize);
        Self::copy_region_handles(
            &shader_group_handle_data,
            handle_size as usize,
            miss_groups,
            &sbt_buffer.miss,
            miss_host_address,
        );

        let hit_host_address = miss_host_address.wrapping_byte_add(miss_size as usize);
        Self::copy_region_handles(
            &shader_group_handle_data,
            handle_size as usize,
            hit_groups,
            &sbt_buffer.hit,
            hit_host_address,
        );

        let callable_host_address = hit_host_address.wrapping_byte_add(hit_size as usize);
        Self::copy_region_handles(
            &shader_group_handle_data,
            handle_size as usize,
            callable_groups,
            &sbt_buffer.callable,
            callable_host_address,
        );

        sbt_buffer.flush(resource_ctx, 0, sbt_buffer.size());
        sbt_buffer
    }

    #[inline]
    pub fn raygen_region(&self) -> &vk::StridedDeviceAddressRegionKHR {
        &self.raygen
    }

    #[inline]
    pub fn miss_region(&self) -> &vk::StridedDeviceAddressRegionKHR {
        &self.miss
    }

    #[inline]
    pub fn hit_region(&self) -> &vk::StridedDeviceAddressRegionKHR {
        &self.hit
    }

    #[inline]
    pub fn callable_region(&self) -> &vk::StridedDeviceAddressRegionKHR {
        &self.callable
    }

    fn region_size(group_count: usize, handle_stride: u32, base_alignment: u32) -> u32 {
        if group_count == 0 { 0 } else { (group_count as u32 * handle_stride).next_multiple_of(base_alignment) }
    }

    fn region(
        sbt_address: vk::DeviceAddress,
        offset: vk::DeviceSize,
        stride: u32,
        size: u32,
    ) -> vk::StridedDeviceAddressRegionKHR {
        if size == 0 {
            vk::StridedDeviceAddressRegionKHR::default()
        } else {
            vk::StridedDeviceAddressRegionKHR::default()
                .stride(stride as vk::DeviceSize)
                .size(size as vk::DeviceSize)
                .device_address(sbt_address + offset)
        }
    }

    fn assert_group_index(group_count: u32, group_index: usize, region_name: &str) {
        assert!(
            group_index < group_count as usize,
            "SBT {region_name} shader group index {group_index} must be less than group_count {group_count}",
        );
    }

    fn assert_group_indices(group_count: u32, group_indices: &[usize], region_name: &str) {
        for group_index in group_indices {
            Self::assert_group_index(group_count, *group_index, region_name);
        }
    }

    fn copy_region_handles(
        shader_group_handle_data: &[u8],
        handle_size: usize,
        group_indices: &[usize],
        region: &vk::StridedDeviceAddressRegionKHR,
        region_host_address: *mut u8,
    ) {
        for (idx, group_index) in group_indices.iter().enumerate() {
            Self::copy_group_handle(
                shader_group_handle_data,
                handle_size,
                *group_index,
                region_host_address.wrapping_byte_add(idx * region.stride as usize),
            );
        }
    }

    fn copy_group_handle(
        shader_group_handle_data: &[u8],
        handle_size: usize,
        group_index: usize,
        sbt_handle_host_address: *mut u8,
    ) {
        let start_bytes = handle_size * group_index;
        let src = &shader_group_handle_data[start_bytes..start_bytes + handle_size];

        unsafe {
            let dst = std::slice::from_raw_parts_mut(sbt_handle_host_address, handle_size);
            dst.copy_from_slice(src);
        }
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
