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
pub(super) struct SceneBuffers {
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
    pub(super) sky_brightness: f32,
    /// 扩容已更换 device 地址但尚未确认完整上传；prepare 中断时也必须保留。
    needs_full_upload: bool,
}

impl SceneBuffers {
    /// 创建一个 FIF frame label 独占的 scene buffer 集。
    ///
    /// 初始容量只影响预分配；prepare 根据本帧实际数据扩容，不约束场景大小。
    pub(super) fn new(ctx: GfxResourceCtx<'_>, frame_label: FrameLabel) -> Self {
        let max_geometry_cnt = 1024 * 8;
        let max_instance_cnt = 1024;

        let scene;
        let geometry;
        let instance;
        let material_indirect;
        let geometry_indirect;
        scene = GfxStructuredBuffer::new_ubo(ctx, 1, format!("scene buffer-{frame_label}"));
        geometry = Self::new_pair(ctx, max_geometry_cnt, &format!("geometry-{frame_label}"));
        instance = Self::new_pair(ctx, max_instance_cnt, &format!("instance-{frame_label}"));
        material_indirect = Self::new_pair(ctx, max_instance_cnt * 8, &format!("material-indirect-{frame_label}"));
        geometry_indirect = Self::new_pair(ctx, max_instance_cnt * 8, &format!("geometry-indirect-{frame_label}"));

        let (geometry_buffer, geometry_stage_buffer) = geometry;
        let (instance_buffer, instance_stage_buffer) = instance;
        let (material_indirect_buffer, material_indirect_stage_buffer) = material_indirect;
        let (geometry_indirect_buffer, geometry_indirect_stage_buffer) = geometry_indirect;
        Self {
            scene_buffer: scene,
            geometry_buffer,
            geometry_stage_buffer,
            instance_buffer,
            instance_stage_buffer,
            material_indirect_buffer,
            material_indirect_stage_buffer,
            geometry_indirect_buffer,
            geometry_indirect_stage_buffer,
            accum_signature: RenderSceneAccumSignature::default(),
            sky_brightness: 1.0,
            needs_full_upload: false,
        }
    }

    /// device 与 stage 必须成对交付，第二个创建失败时撤销第一个。
    fn new_pair<T>(
        ctx: GfxResourceCtx<'_>,
        capacity: usize,
        name: &str,
    ) -> (GfxStructuredBuffer<T>, GfxStructuredBuffer<T>) {
        let device = GfxStructuredBuffer::new_ssbo(ctx, capacity, name);
        let stage = { GfxStructuredBuffer::new_stage_buffer(ctx, capacity, format!("{name}-stage")) };
        (device, stage)
    }

    /// 调用方必须先确认 GPU 对两个 buffer 的使用均已结束。
    fn destroy_pair<T>(
        device: &mut GfxStructuredBuffer<T>,
        stage: &mut GfxStructuredBuffer<T>,
        ctx: GfxResourceCtx<'_>,
        reason: DestroyReason,
    ) {
        device.destroy_mut(ctx, reason);
        stage.destroy_mut(ctx, reason);
    }

    /// 当前 FIF 的上次提交已经由 Runtime 等待完成，才允许替换这些 buffer。
    ///
    /// instance 容量按最高稳定 slot 计算，indirect 容量按 active submesh 数计算；两者不能
    /// 混用。扩容保留 slot 身份，随后整表上传并更新 scene root，不影响其它在飞帧。
    /// 返回值包含先前中断留下的扩容标记，直到 commit_full_upload 确认才清除。
    pub(super) fn ensure_capacity(
        &mut self,
        ctx: GfxResourceCtx<'_>,
        frame_label: FrameLabel,
        data: &super::render_data::RenderData<'_>,
    ) -> bool {
        let (geometry_count, instance_count, indirect_count) = data.buffer_counts();
        let frame = frame_label;

        Self::grow_pair(
            &mut self.geometry_buffer,
            &mut self.geometry_stage_buffer,
            geometry_count,
            &format!("geometry-{frame}"),
            ctx,
            &mut self.needs_full_upload,
        );
        Self::grow_pair(
            &mut self.instance_buffer,
            &mut self.instance_stage_buffer,
            instance_count,
            &format!("instance-{frame}"),
            ctx,
            &mut self.needs_full_upload,
        );
        Self::grow_pair(
            &mut self.material_indirect_buffer,
            &mut self.material_indirect_stage_buffer,
            indirect_count,
            &format!("material-indirect-{frame}"),
            ctx,
            &mut self.needs_full_upload,
        );
        Self::grow_pair(
            &mut self.geometry_indirect_buffer,
            &mut self.geometry_indirect_stage_buffer,
            indirect_count,
            &format!("geometry-indirect-{frame}"),
            ctx,
            &mut self.needs_full_upload,
        );
        self.needs_full_upload
    }

    /// 仅在当前 FIF 的完整结构上传已成功提交后调用；清除标记不等于 GPU 执行结束。
    pub(super) fn commit_full_upload(&mut self) {
        self.needs_full_upload = false;
    }

    fn grow_pair<T: bytemuck::Pod>(
        device: &mut GfxStructuredBuffer<T>,
        stage: &mut GfxStructuredBuffer<T>,
        required: usize,
        name: &str,
        ctx: GfxResourceCtx<'_>,
        needs_full_upload: &mut bool,
    ) {
        if required <= stage.mapped_slice_ref().len() {
            return;
        }
        let capacity = required.checked_next_power_of_two().expect("scene buffer capacity overflow");
        log::info!("Scene buffer {name} growing to {capacity} elements");
        let (next_device, next_stage) = Self::new_pair(ctx, capacity, name);
        // 当前 FIF 已等待完成；先让字段接管完整候选，再回收旧 pair，诊断失败不留下悬空字段。
        let mut old_device = std::mem::replace(device, next_device);
        let mut old_stage = std::mem::replace(stage, next_stage);
        *needs_full_upload = true;
        Self::destroy_pair(&mut old_device, &mut old_stage, ctx, DestroyReason::ImmediateRelease);
    }

    /// 销毁该 FIF 的全部 scene buffer。
    pub(super) fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>, _device_ctx: GfxDeviceCtx<'_>) {
        self.scene_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        Self::destroy_pair(
            &mut self.geometry_buffer,
            &mut self.geometry_stage_buffer,
            resource_ctx,
            DestroyReason::Shutdown,
        );
        Self::destroy_pair(
            &mut self.instance_buffer,
            &mut self.instance_stage_buffer,
            resource_ctx,
            DestroyReason::Shutdown,
        );
        Self::destroy_pair(
            &mut self.material_indirect_buffer,
            &mut self.material_indirect_stage_buffer,
            resource_ctx,
            DestroyReason::Shutdown,
        );
        Self::destroy_pair(
            &mut self.geometry_indirect_buffer,
            &mut self.geometry_indirect_stage_buffer,
            resource_ctx,
            DestroyReason::Shutdown,
        );
    }
}
