use truvis_gfx::resources::special_buffers::index_buffer::GfxIndexBuffer;
use truvis_gfx::resources::special_buffers::vertex_buffer::GfxVertexBuffer;
use truvis_render_interface::pipeline_settings::FrameLabel;

use crate::gui_vertex_layout::{ImGuiVertex, ImGuiVertexLayoutAoS};

/// imgui 绘制所需的 vertex buffer 和 index buffer
pub struct GuiMesh {
    pub vertex_buffer: GfxVertexBuffer<ImGuiVertexLayoutAoS>,
    pub vertex_count: usize,

    pub index_buffer: GfxIndexBuffer<imgui::DrawIdx>,
    pub index_count: usize,

    frame_label: FrameLabel,
}

impl GuiMesh {
    pub fn new(frame_label: FrameLabel) -> Self {
        // 初始大小为 64KB
        let vertex_count = 64 * 1024 / size_of::<ImGuiVertex>();
        // 初始大小为 96KB
        let index_count = 96 * 1024 / size_of::<imgui::DrawIdx>();

        Self {
            vertex_count,
            index_count,
            vertex_buffer: Self::new_vertex_buffer(frame_label, vertex_count),
            index_buffer: Self::new_index_buffer(frame_label, index_count),
            frame_label,
        }
    }

    fn new_vertex_buffer(frame_label: FrameLabel, vertex_cnt: usize) -> GfxVertexBuffer<ImGuiVertexLayoutAoS> {
        GfxVertexBuffer::<ImGuiVertexLayoutAoS>::new(vertex_cnt, true, format!("imgui-vertex-{}", frame_label))
    }

    fn new_index_buffer(frame_label: FrameLabel, index_cnt: usize) -> GfxIndexBuffer<imgui::DrawIdx> {
        GfxIndexBuffer::<imgui::DrawIdx>::new(index_cnt, true, format!("imgui-index-{}", frame_label))
    }

    /// 根据 draw data 的需求，动态增长 buffer 大小
    pub fn grow_if_needed(&mut self, draw_data: &imgui::DrawData) {
        if (draw_data.total_vtx_count as usize) > self.vertex_count {
            self.vertex_count = (draw_data.total_vtx_count as usize).next_power_of_two();
            self.vertex_buffer = Self::new_vertex_buffer(self.frame_label, self.vertex_count);
        }

        if (draw_data.total_idx_count as usize) > self.index_count {
            self.index_count = (draw_data.total_idx_count as usize).next_power_of_two();
            self.index_buffer = Self::new_index_buffer(self.frame_label, self.index_count);
        }
    }

    /// 从 draw data 中提取出 vertex 数据，更新 vertex buffer
    pub fn fill_vertex_buffer(&mut self, draw_data: &imgui::DrawData) {
        let mut vertices = Vec::with_capacity(draw_data.total_vtx_count as usize);
        for draw_list in draw_data.draw_lists() {
            vertices.extend_from_slice(draw_list.vtx_buffer());
        }
        self.vertex_buffer.transfer_data_by_mmap(&vertices);
    }

    /// 从 draw data 中提取出 index 数据，更新到 index buffer
    pub fn fill_index_buffer(&mut self, draw_data: &imgui::DrawData) {
        let mut indices = Vec::with_capacity(draw_data.total_idx_count as usize);
        for draw_list in draw_data.draw_lists() {
            indices.extend_from_slice(draw_list.idx_buffer());
        }

        self.index_buffer.transfer_data_by_mmap(&indices);
    }
}
