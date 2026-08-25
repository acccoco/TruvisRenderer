use truvis_gfx::gfx::{GfxDeviceCtx, GfxResourceCtx};
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_render_foundation::render_scene_view::RenderSceneAccumSignature;
use truvis_shader_binding::gpu;

/// 构建 render-side scene 所需的 per-FIF buffer 集。
///
/// 每个 frame label 拥有独立的 scene/instance/geometry/material-indirect buffer，
/// 避免 CPU 准备下一帧数据时覆盖 GPU 仍在读取的上一帧 buffer。
pub(super) struct RenderWorldBuffers {
    /// scene root UBO，保存 shader 访问其它 scene buffer 的 device address 与 bindless handle。
    pub(super) scene_buffer: GfxStructuredBuffer<gpu::engine::scene::GpuScene>,
    /// geometry table device buffer，元素只保存 vertex/index buffer device address。
    pub(super) geometry_buffer: GfxStructuredBuffer<gpu::engine::geometry::Geometry>,
    pub(super) geometry_stage_buffer: GfxStructuredBuffer<gpu::engine::geometry::Geometry>,
    /// 稳定 instance slot 索引的 device buffer，TLAS custom index 与它共享同一语义。
    pub(super) instance_buffer: GfxStructuredBuffer<gpu::engine::scene::Instance>,
    pub(super) instance_stage_buffer: GfxStructuredBuffer<gpu::engine::scene::Instance>,
    /// instance -> material slot 的间接索引表，按本帧 active instance/submesh 紧凑写入。
    pub(super) material_indirect_buffer: GfxStructuredBuffer<u32>,
    pub(super) material_indirect_stage_buffer: GfxStructuredBuffer<u32>,
    /// instance -> geometry table index 的间接索引表，按本帧 active instance/submesh 紧凑写入。
    pub(super) geometry_indirect_buffer: GfxStructuredBuffer<u32>,
    pub(super) geometry_indirect_stage_buffer: GfxStructuredBuffer<u32>,

    /// 当前 FIF 的 scene 语义版本快照，供离线累计判断历史图像是否仍可复用。
    pub(super) accum_signature: RenderSceneAccumSignature,
}

impl RenderWorldBuffers {
    /// 创建一个 FIF frame label 独占的 scene buffer 集。
    ///
    /// 固定容量与 `RenderInstanceManager` 等上游桥接层的 slot 上限保持一致；容量不足时上传阶段
    /// 会显式 panic，便于暴露当前后端还没有动态扩容的限制。
    pub(super) fn new(ctx: GfxResourceCtx<'_>, frame_label: FrameLabel) -> Self {
        let max_geometry_cnt = 1024 * 8;
        let max_instance_cnt = 1024;

        RenderWorldBuffers {
            scene_buffer: GfxStructuredBuffer::new_ubo(ctx, 1, format!("scene buffer-{}", frame_label)),
            geometry_buffer: GfxStructuredBuffer::new_ssbo(
                ctx,
                max_geometry_cnt,
                format!("geometry buffer-{}", frame_label),
            ),
            geometry_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                ctx,
                max_geometry_cnt,
                format!("geometry stage buffer-{}", frame_label),
            ),
            instance_buffer: GfxStructuredBuffer::new_ssbo(
                ctx,
                max_instance_cnt,
                format!("instance buffer-{}", frame_label),
            ),
            instance_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                ctx,
                max_instance_cnt,
                format!("instance stage buffer-{}", frame_label),
            ),
            material_indirect_buffer: GfxStructuredBuffer::new_ssbo(
                ctx,
                max_instance_cnt * 8,
                format!("instance material buffer-{}", frame_label),
            ),
            material_indirect_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                ctx,
                max_instance_cnt * 8,
                format!("instance material stage buffer-{}", frame_label),
            ),
            geometry_indirect_buffer: GfxStructuredBuffer::new_ssbo(
                ctx,
                max_instance_cnt * 8,
                format!("instance geometry buffer-{}", frame_label),
            ),
            geometry_indirect_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                ctx,
                max_instance_cnt * 8,
                format!("instance geometry stage buffer-{}", frame_label),
            ),
            accum_signature: RenderSceneAccumSignature::default(),
        }
    }

    /// 销毁该 FIF 的全部 scene buffer。
    pub(super) fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>, _device_ctx: GfxDeviceCtx<'_>) {
        self.scene_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.geometry_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.geometry_stage_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.instance_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.instance_stage_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.material_indirect_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.material_indirect_stage_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.geometry_indirect_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.geometry_indirect_stage_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
    }
}
