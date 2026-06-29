use ash::vk;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::resources::layout::GfxVertexLayout;
use truvis_gfx::resources::vertex_layout::soa_3d::VertexLayoutSoA3D;

use super::render_data::RenderData;

/// 光栅化 pass 的轻量 draw cache。
///
/// `RenderWorld` 在 prepare 阶段从 `RenderData` 展开出每个 submesh 的 buffer 绑定信息，
/// render pass 只遍历这个 cache，不再接触 scene/asset bridge。
#[derive(Clone, Copy)]
pub(super) struct RasterDrawItem {
    index_buffer: vk::Buffer,
    index_count: u32,
    vertex_buffers: [vk::Buffer; 4],
    vertex_offsets: [vk::DeviceSize; 4],
    instance_slot: u32,
    submesh_idx: u32,
}

pub(super) fn update_raster_draw_cache(draw_cache: &mut Vec<RasterDrawItem>, scene_data: &RenderData<'_>) {
    // 光栅化绘制需要的 buffer 绑定在 prepare 阶段展开，避免 render pass 每次 draw
    // 再走 mesh/instance/material 的跨模块解析。
    draw_cache.clear();

    for instance in scene_data.all_instances.iter() {
        let mesh = &scene_data.all_meshes[instance.mesh_index];
        for (submesh_idx, geometry) in mesh.geometries.iter().enumerate() {
            let vertex_count = geometry.vertex_buffer.vertex_cnt();
            draw_cache.push(RasterDrawItem {
                index_buffer: geometry.index_buffer.vk_buffer(),
                index_count: geometry.index_cnt(),
                vertex_buffers: [geometry.vertex_buffer.vk_buffer(); 4],
                vertex_offsets: [
                    VertexLayoutSoA3D::pos_offset(vertex_count),
                    VertexLayoutSoA3D::normal_offset(vertex_count),
                    VertexLayoutSoA3D::tangent_offset(vertex_count),
                    VertexLayoutSoA3D::uv_offset(vertex_count),
                ],
                instance_slot: instance.instance_slot.as_u32(),
                submesh_idx: submesh_idx as u32,
            });
        }
    }
}

pub(super) fn draw_raster_cache(
    draw_cache: &[RasterDrawItem],
    cmd: &GfxCommandBuffer,
    before_draw: &mut dyn FnMut(u32, u32),
) {
    // render pass 只获得只读 view。每次 draw 前回调 instance slot/submesh index，
    // 让具体 pass 能绑定 push constants 或 descriptor，而不暴露 RenderWorld 内部缓存结构。
    for draw in draw_cache {
        cmd.cmd_bind_index_buffer_raw(draw.index_buffer, 0, super::geometry::RtGeometry::index_type());
        cmd.cmd_bind_vertex_buffers(0, &draw.vertex_buffers, &draw.vertex_offsets);

        before_draw(draw.instance_slot, draw.submesh_idx);
        cmd.draw_indexed(draw.index_count, 0, 1, 0, 0);
    }
}
