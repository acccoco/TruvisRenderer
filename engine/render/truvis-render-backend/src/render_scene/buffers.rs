use truvis_gfx::gfx::{GfxDeviceCtx, GfxResourceCtx};
use truvis_gfx::raytracing::acceleration::GfxAcceleration;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_interface::pipeline_settings::FrameLabel;
use truvis_shader_binding::gpu;

/// 构建 GPU scene 所需的 per-FIF buffer 集。
///
/// 每个 frame label 拥有独立的 scene/instance/geometry/light/material-indirect buffer，
/// 避免 CPU 准备下一帧数据时覆盖 GPU 仍在读取的上一帧 buffer。
pub(super) struct GpuSceneBuffers {
    pub(super) scene_buffer: GfxStructuredBuffer<gpu::GPUScene>,
    pub(super) light_buffer: GfxStructuredBuffer<gpu::PointLight>,
    pub(super) light_stage_buffer: GfxStructuredBuffer<gpu::PointLight>,
    pub(super) geometry_buffer: GfxStructuredBuffer<gpu::Geometry>,
    pub(super) geometry_stage_buffer: GfxStructuredBuffer<gpu::Geometry>,
    pub(super) instance_buffer: GfxStructuredBuffer<gpu::Instance>,
    pub(super) instance_stage_buffer: GfxStructuredBuffer<gpu::Instance>,
    pub(super) material_indirect_buffer: GfxStructuredBuffer<u32>,
    pub(super) material_indirect_stage_buffer: GfxStructuredBuffer<u32>,
    pub(super) geometry_indirect_buffer: GfxStructuredBuffer<u32>,
    pub(super) geometry_indirect_stage_buffer: GfxStructuredBuffer<u32>,

    // TODO 使用 frame id 来标记是否过期，scene manager 里面也需要有相应的标记
    pub(super) tlas: Option<GfxAcceleration>,
    pub(super) tlas_revision: u64,
}

impl GpuSceneBuffers {
    pub(super) fn new(ctx: GfxResourceCtx<'_>, frame_label: FrameLabel) -> Self {
        let max_light_cnt = 512;
        let max_geometry_cnt = 1024 * 8;
        let max_instance_cnt = 1024;

        GpuSceneBuffers {
            scene_buffer: GfxStructuredBuffer::new_ubo(ctx, 1, format!("scene buffer-{}", frame_label)),
            light_buffer: GfxStructuredBuffer::new_ssbo(ctx, max_light_cnt, format!("light buffer-{}", frame_label)),
            light_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                ctx,
                max_light_cnt,
                format!("light stage buffer-{}", frame_label),
            ),
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
            tlas: None,
            tlas_revision: 0,
        }
    }

    pub(super) fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        if let Some(tlas) = self.tlas.take() {
            tlas.destroy(resource_ctx, device_ctx, DestroyReason::Shutdown);
        }
        self.scene_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.light_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.light_stage_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
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
