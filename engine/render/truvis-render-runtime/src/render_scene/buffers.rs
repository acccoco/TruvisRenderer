use truvis_gfx::gfx::{GfxDeviceCtx, GfxResourceCtx};
use truvis_gfx::raytracing::acceleration::GfxAcceleration;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_foundation::frame_counter::FrameLabel;
use truvis_shader_binding::gpu;

/// 构建 GPU scene 所需的 per-FIF buffer 集。
///
/// 每个 frame label 拥有独立的 scene/instance/geometry/light/material-indirect buffer，
/// 避免 CPU 准备下一帧数据时覆盖 GPU 仍在读取的上一帧 buffer。
pub(super) struct GpuSceneBuffers {
    /// scene root UBO，保存 shader 访问其它 scene buffer 的 device address 与 bindless handle。
    pub(super) scene_buffer: GfxStructuredBuffer<gpu::scene::GpuScene>,
    /// point light device buffer，与 `point_light_stage_buffer` 成对使用。
    pub(super) point_light_buffer: GfxStructuredBuffer<gpu::light::PointLight>,
    pub(super) point_light_stage_buffer: GfxStructuredBuffer<gpu::light::PointLight>,
    /// spot light device buffer，与 `spot_light_stage_buffer` 成对使用。
    pub(super) spot_light_buffer: GfxStructuredBuffer<gpu::light::SpotLight>,
    pub(super) spot_light_stage_buffer: GfxStructuredBuffer<gpu::light::SpotLight>,
    /// area light device buffer，与 `area_light_stage_buffer` 成对使用。
    pub(super) area_light_buffer: GfxStructuredBuffer<gpu::light::AreaLight>,
    pub(super) area_light_stage_buffer: GfxStructuredBuffer<gpu::light::AreaLight>,
    /// geometry table device buffer，元素只保存 vertex/index buffer device address。
    pub(super) geometry_buffer: GfxStructuredBuffer<gpu::geometry::Geometry>,
    pub(super) geometry_stage_buffer: GfxStructuredBuffer<gpu::geometry::Geometry>,
    /// 稳定 instance slot 索引的 device buffer，TLAS custom index 与它共享同一语义。
    pub(super) instance_buffer: GfxStructuredBuffer<gpu::scene::Instance>,
    pub(super) instance_stage_buffer: GfxStructuredBuffer<gpu::scene::Instance>,
    /// instance -> material slot 的间接索引表，按本帧 active instance/submesh 紧凑写入。
    pub(super) material_indirect_buffer: GfxStructuredBuffer<u32>,
    pub(super) material_indirect_stage_buffer: GfxStructuredBuffer<u32>,
    /// instance -> geometry table index 的间接索引表，按本帧 active instance/submesh 紧凑写入。
    pub(super) geometry_indirect_buffer: GfxStructuredBuffer<u32>,
    pub(super) geometry_indirect_stage_buffer: GfxStructuredBuffer<u32>,

    // TODO 使用 frame id 来标记是否过期，scene manager 里面也需要有相应的标记
    pub(super) tlas: Option<GfxAcceleration>,
    pub(super) tlas_revision: u64,
}

impl GpuSceneBuffers {
    /// 创建一个 FIF frame label 独占的 scene buffer 集。
    ///
    /// 固定容量与 `InstanceBridge` 等上游桥接层的 slot 上限保持一致；容量不足时上传阶段
    /// 会显式 panic，便于暴露当前后端还没有动态扩容的限制。
    pub(super) fn new(ctx: GfxResourceCtx<'_>, frame_label: FrameLabel) -> Self {
        let max_light_cnt = 512;
        let max_geometry_cnt = 1024 * 8;
        let max_instance_cnt = 1024;

        GpuSceneBuffers {
            scene_buffer: GfxStructuredBuffer::new_ubo(ctx, 1, format!("scene buffer-{}", frame_label)),
            point_light_buffer: GfxStructuredBuffer::new_ssbo(
                ctx,
                max_light_cnt,
                format!("point light buffer-{}", frame_label),
            ),
            point_light_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                ctx,
                max_light_cnt,
                format!("point light stage buffer-{}", frame_label),
            ),
            spot_light_buffer: GfxStructuredBuffer::new_ssbo(
                ctx,
                max_light_cnt,
                format!("spot light buffer-{}", frame_label),
            ),
            spot_light_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                ctx,
                max_light_cnt,
                format!("spot light stage buffer-{}", frame_label),
            ),
            area_light_buffer: GfxStructuredBuffer::new_ssbo(
                ctx,
                max_light_cnt,
                format!("area light buffer-{}", frame_label),
            ),
            area_light_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                ctx,
                max_light_cnt,
                format!("area light stage buffer-{}", frame_label),
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

    /// 销毁该 FIF 的全部 scene buffer 与 TLAS。
    pub(super) fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        if let Some(tlas) = self.tlas.take() {
            tlas.destroy(resource_ctx, device_ctx, DestroyReason::Shutdown);
        }
        self.scene_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.point_light_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.point_light_stage_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.spot_light_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.spot_light_stage_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.area_light_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.area_light_stage_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
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
