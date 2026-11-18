use std::ptr;

use ash::vk;

use crate::gfx::{GfxImmediateCtx, GfxResourceCtx};
use crate::resources::layout::GfxVertexLayout;
use crate::resources::special_buffers::vertex_buffer::GfxVertexBuffer;

/// SoA 的顶点 buffer 布局，包含：Positions, Normals, Tangents, UVs
pub struct VertexLayoutSoA3D;
impl GfxVertexLayout for VertexLayoutSoA3D {
    fn vertex_input_bindings() -> Vec<vk::VertexInputBindingDescription> {
        vec![
            // 位置
            vk::VertexInputBindingDescription {
                binding: 0,
                stride: size_of::<glam::Vec3>() as u32,
                input_rate: vk::VertexInputRate::VERTEX,
            },
            // 法线
            vk::VertexInputBindingDescription {
                binding: 1,
                stride: size_of::<glam::Vec3>() as u32,
                input_rate: vk::VertexInputRate::VERTEX,
            },
            // 切线
            vk::VertexInputBindingDescription {
                binding: 2,
                stride: size_of::<glam::Vec4>() as u32,
                input_rate: vk::VertexInputRate::VERTEX,
            },
            // UV 坐标
            vk::VertexInputBindingDescription {
                binding: 3,
                stride: size_of::<glam::Vec2>() as u32,
                input_rate: vk::VertexInputRate::VERTEX,
            },
        ]
    }

    fn vertex_input_attributes() -> Vec<vk::VertexInputAttributeDescription> {
        vec![
            // 位置
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 0,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: 0,
            },
            // 法线
            vk::VertexInputAttributeDescription {
                binding: 1,
                location: 1,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: 0,
            },
            // 切线
            vk::VertexInputAttributeDescription {
                binding: 2,
                location: 2,
                format: vk::Format::R32G32B32A32_SFLOAT,
                offset: 0,
            },
            // UV 坐标
            vk::VertexInputAttributeDescription {
                binding: 3,
                location: 3,
                format: vk::Format::R32G32_SFLOAT,
                offset: 0,
            },
        ]
    }

    fn buffer_size(vertex_cnt: usize) -> usize {
        let size = usize::try_from(Self::uv_offset(vertex_cnt))
            .expect("SoA UV offset exceeds host range")
            .checked_add(vertex_cnt.checked_mul(size_of::<glam::Vec2>()).expect("SoA UV size overflow"))
            .expect("SoA buffer size overflow");
        assert!(size <= isize::MAX as usize, "SoA buffer exceeds host addressable range");
        size
    }
    fn pos_stride() -> u32 {
        size_of::<glam::Vec3>() as u32
    }
    fn pos_offset(_vertex_cnt: usize) -> vk::DeviceSize {
        0
    }
    fn normal_offset(vertex_cnt: usize) -> vk::DeviceSize {
        vertex_cnt.checked_mul(size_of::<glam::Vec3>()).expect("SoA normal offset overflow") as vk::DeviceSize
    }
    fn tangent_offset(vertex_cnt: usize) -> vk::DeviceSize {
        vertex_cnt
            .checked_mul(size_of::<glam::Vec3>() * 2)
            .and_then(|size| size.checked_next_multiple_of(16))
            .expect("SoA tangent offset overflow") as vk::DeviceSize
    }
    fn uv_offset(vertex_cnt: usize) -> vk::DeviceSize {
        Self::tangent_offset(vertex_cnt)
            .checked_add(vertex_cnt.checked_mul(size_of::<glam::Vec4>()).expect("SoA tangent size overflow") as u64)
            .expect("SoA UV offset overflow")
    }
}

impl VertexLayoutSoA3D {
    /// 固定布局已包含一个 UV 槽；额外集合连续追加，device/staging 分配复用同一尺寸契约。
    pub fn extra_uv_bytes(vertex_cnt: usize, uv_set_count: usize) -> usize {
        let extra = uv_set_count
            .saturating_sub(1)
            .checked_mul(vertex_cnt)
            .and_then(|count| count.checked_mul(size_of::<glam::Vec2>()))
            .expect("SoA extra UV size overflow");
        let total = Self::buffer_size(vertex_cnt).checked_add(extra).expect("SoA total size overflow");
        assert!(total <= isize::MAX as usize, "SoA buffer exceeds host addressable range");
        extra
    }

    pub fn create_vertex_buffer(
        resource_ctx: GfxResourceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        positions: &[glam::Vec3],
        normals: &[glam::Vec3],
        tangents: &[glam::Vec4],
        uvs: &[glam::Vec2],
        name: impl AsRef<str>,
    ) -> GfxVertexBuffer<Self> {
        let vertex_cnt = positions.len();
        assert!(vertex_cnt == normals.len() && vertex_cnt == tangents.len() && vertex_cnt == uvs.len());

        let mut vertex_buffer = GfxVertexBuffer::new_device_local(resource_ctx, vertex_cnt, name.as_ref());
        vertex_buffer.transfer_data_sync2(
            resource_ctx,
            immediate_ctx,
            Self::buffer_size(vertex_cnt) as vk::DeviceSize,
            |stage_buffer| unsafe {
                ptr::copy_nonoverlapping(
                    positions.as_ptr() as *const u8,
                    stage_buffer.mapped_ptr().add(Self::pos_offset(vertex_cnt) as usize),
                    size_of_val(positions),
                );
                ptr::copy_nonoverlapping(
                    normals.as_ptr() as *const u8,
                    stage_buffer.mapped_ptr().add(Self::normal_offset(vertex_cnt) as usize),
                    size_of_val(normals),
                );
                ptr::copy_nonoverlapping(
                    tangents.as_ptr() as *const u8,
                    stage_buffer.mapped_ptr().add(Self::tangent_offset(vertex_cnt) as usize),
                    size_of_val(tangents),
                );
                ptr::copy_nonoverlapping(
                    uvs.as_ptr() as *const u8,
                    stage_buffer.mapped_ptr().add(Self::uv_offset(vertex_cnt) as usize),
                    size_of_val(uvs),
                );
            },
        );

        vertex_buffer
    }
}
