use std::mem::offset_of;

use ash::vk;
use imgui::DrawVert;

use truvis_gfx::resources::layout::GfxVertexLayout;

/// GuiMesh 直接上传 DrawVert；stride 和 attribute offset 必须来自同一类型。
pub struct ImGuiVertexLayoutAoS;
impl GfxVertexLayout for ImGuiVertexLayoutAoS {
    fn vertex_input_bindings() -> Vec<vk::VertexInputBindingDescription> {
        vec![vk::VertexInputBindingDescription {
            binding: 0,
            stride: size_of::<DrawVert>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }]
    }

    fn vertex_input_attributes() -> Vec<vk::VertexInputAttributeDescription> {
        vec![
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: offset_of!(DrawVert, pos) as u32,
            },
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 1,
                format: vk::Format::R32G32_SFLOAT,
                offset: offset_of!(DrawVert, uv) as u32,
            },
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 2,
                format: vk::Format::R8G8B8A8_UNORM,
                offset: offset_of!(DrawVert, col) as u32,
            },
        ]
    }

    fn buffer_size(vertex_cnt: usize) -> usize {
        vertex_cnt * size_of::<DrawVert>()
    }
}
