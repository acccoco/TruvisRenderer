use ash::vk;
use std::ptr;

use crate::resources::layout::GfxVertexLayout;
use crate::resources::special_buffers::vertex_buffer::GfxVertexBuffer;

/// SoA 的顶点 buffer 布局，包含：Positions, Normals, Tangents, UVs
pub struct VertexLayoutSoA3D;
impl GfxVertexLayout for VertexLayoutSoA3D {
    fn vertex_input_bindings() -> Vec<vk::VertexInputBindingDescription> {
        vec![
            // positions
            vk::VertexInputBindingDescription {
                binding: 0,
                stride: size_of::<glam::Vec3>() as u32,
                input_rate: vk::VertexInputRate::VERTEX,
            },
            // normals
            vk::VertexInputBindingDescription {
                binding: 1,
                stride: size_of::<glam::Vec3>() as u32,
                input_rate: vk::VertexInputRate::VERTEX,
            },
            // tangents
            vk::VertexInputBindingDescription {
                binding: 2,
                stride: size_of::<glam::Vec3>() as u32,
                input_rate: vk::VertexInputRate::VERTEX,
            },
            // uvs
            vk::VertexInputBindingDescription {
                binding: 3,
                stride: size_of::<glam::Vec2>() as u32,
                input_rate: vk::VertexInputRate::VERTEX,
            },
        ]
    }

    fn vertex_input_attributes() -> Vec<vk::VertexInputAttributeDescription> {
        vec![
            // positions
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 0,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: 0,
            },
            // normals
            vk::VertexInputAttributeDescription {
                binding: 1,
                location: 1,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: 0,
            },
            // tangents
            vk::VertexInputAttributeDescription {
                binding: 2,
                location: 2,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: 0,
            },
            // uvs
            vk::VertexInputAttributeDescription {
                binding: 3,
                location: 3,
                format: vk::Format::R32G32_SFLOAT,
                offset: 0,
            },
        ]
    }

    fn buffer_size(vertex_cnt: usize) -> usize {
        vertex_cnt * (size_of::<glam::Vec3>() * 3 + size_of::<glam::Vec2>())
    }
    fn pos_stride() -> u32 {
        size_of::<glam::Vec3>() as u32
    }
    fn pos_offset(_vertex_cnt: usize) -> vk::DeviceSize {
        0
    }
    fn normal_offset(vertex_cnt: usize) -> vk::DeviceSize {
        (vertex_cnt * size_of::<glam::Vec3>()) as vk::DeviceSize
    }
    fn tangent_offset(vertex_cnt: usize) -> vk::DeviceSize {
        (vertex_cnt * size_of::<glam::Vec3>() * 2) as vk::DeviceSize
    }
    fn uv_offset(vertex_cnt: usize) -> vk::DeviceSize {
        (vertex_cnt * (size_of::<glam::Vec3>() * 3)) as vk::DeviceSize
    }
}

impl VertexLayoutSoA3D {
    pub fn create_vertex_buffer(
        positions: &[glam::Vec3],
        normals: &[glam::Vec3],
        tangents: &[glam::Vec3],
        uvs: &[glam::Vec2],
        name: impl AsRef<str>,
    ) -> GfxVertexBuffer<Self> {
        let vertex_cnt = positions.len();
        assert!(vertex_cnt == normals.len() && vertex_cnt == tangents.len() && vertex_cnt == uvs.len());

        let vertex_buffer = GfxVertexBuffer::new_device_local(vertex_cnt, name.as_ref());
        vertex_buffer.transfer_data_sync2(Self::buffer_size(vertex_cnt) as vk::DeviceSize, |stage_buffer| unsafe {
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
        });

        vertex_buffer
    }
}
