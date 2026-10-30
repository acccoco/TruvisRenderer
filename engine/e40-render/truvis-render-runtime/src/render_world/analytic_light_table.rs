use ash::vk;

use truvis_gfx::commands::barrier::{GfxBarrierMask, GfxBufferBarrier};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_shader_binding::gpu;
use truvis_world::SceneReadView;

const INITIAL_ANALYTIC_LIGHT_CAPACITY: usize = 512;

/// 写入 scene root 的 analytic light buffer 绑定快照。
#[derive(Clone, Copy, Default)]
pub(crate) struct AnalyticLightBinding {
    pub(crate) point_lights: vk::DeviceAddress,
    pub(crate) spot_lights: vk::DeviceAddress,
    pub(crate) area_lights: vk::DeviceAddress,
    pub(crate) point_light_count: u32,
    pub(crate) spot_light_count: u32,
    pub(crate) area_light_count: u32,
    pub(crate) version: u32,
}

/// 同类灯光的 device/stage 共同创建和回收，避免候选只完成一半就丢失 owner。
struct AnalyticLightBuffers<T> {
    device: GfxStructuredBuffer<T>,
    stage: GfxStructuredBuffer<T>,
}

impl<T> AnalyticLightBuffers<T> {
    fn new(ctx: GfxResourceCtx<'_>, capacity: usize, name: &str) -> Self {
        Self {
            device: GfxStructuredBuffer::new_ssbo(ctx, capacity, name),
            stage: GfxStructuredBuffer::new_stage_buffer(ctx, capacity, format!("{name}-stage")),
        }
    }

    /// 调用方必须先确认两个 buffer 都已不再被 GPU 使用。
    fn destroy_mut(&mut self, ctx: GfxResourceCtx<'_>, reason: DestroyReason) {
        self.device.destroy_mut(ctx, reason);
        self.stage.destroy_mut(ctx, reason);
    }
}

impl<T: bytemuck::Pod> AnalyticLightBuffers<T> {
    /// 当前 FIF 已等待完成；完整候选先接管字段，再回收旧 pair。
    /// 调用方的 dirty 在整组上传提交确认前始终保留，因此中断后仍会重传新地址的内容。
    fn ensure_capacity(&mut self, ctx: GfxResourceCtx<'_>, required: usize, name: &str) {
        if required <= self.stage.mapped_slice_ref().len() {
            return;
        }
        let capacity = required.checked_next_power_of_two().expect("analytic light capacity overflow");
        let next = Self::new(ctx, capacity, name);
        let mut old = std::mem::replace(self, next);
        old.destroy_mut(ctx, DestroyReason::ImmediateRelease);
    }

    /// 只上传本快照的有效前缀；空类别由 binding 的 count 表达，不访问预分配尾部。
    fn upload(&mut self, ctx: GfxResourceCtx<'_>, cmd: &GfxCommandBuffer, barrier_mask: GfxBarrierMask, data: &[T]) {
        if data.is_empty() {
            return;
        }
        self.stage.mapped_slice()[..data.len()].copy_from_slice(data);
        let copy_size = u64::try_from(std::mem::size_of_val(data)).expect("analytic upload size exceeds u64");
        self.stage.flush(ctx, 0, copy_size);
        cmd.cmd_copy_buffer(
            &self.stage,
            &self.device,
            &[vk::BufferCopy {
                size: copy_size,
                ..Default::default()
            }],
        );
        cmd.buffer_memory_barrier(
            vk::DependencyFlags::empty(),
            &[GfxBufferBarrier::default().mask(barrier_mask).buffer(self.device.vk_buffer(), 0, vk::WHOLE_SIZE)],
        );
    }
}

struct AnalyticLightFrameBuffers {
    point: AnalyticLightBuffers<gpu::engine::light::PointLight>,
    spot: AnalyticLightBuffers<gpu::engine::light::SpotLight>,
    area: AnalyticLightBuffers<gpu::engine::light::AreaLight>,
}

impl AnalyticLightFrameBuffers {
    fn new(ctx: GfxResourceCtx<'_>, frame_label: FrameLabel) -> Self {
        Self {
            point: AnalyticLightBuffers::new(
                ctx,
                INITIAL_ANALYTIC_LIGHT_CAPACITY,
                &format!("point light-{frame_label}"),
            ),
            spot: AnalyticLightBuffers::new(ctx, INITIAL_ANALYTIC_LIGHT_CAPACITY, &format!("spot light-{frame_label}")),
            area: AnalyticLightBuffers::new(ctx, INITIAL_ANALYTIC_LIGHT_CAPACITY, &format!("area light-{frame_label}")),
        }
    }

    fn destroy_mut(&mut self, ctx: GfxResourceCtx<'_>, reason: DestroyReason) {
        self.point.destroy_mut(ctx, reason);
        self.spot.destroy_mut(ctx, reason);
        self.area.destroy_mut(ctx, reason);
    }

    fn binding(
        &self,
        point_light_count: u32,
        spot_light_count: u32,
        area_light_count: u32,
        version: u32,
    ) -> AnalyticLightBinding {
        AnalyticLightBinding {
            point_lights: self.point.device.device_address(),
            spot_lights: self.spot.device.device_address(),
            area_lights: self.area.device.device_address(),
            point_light_count,
            spot_light_count,
            area_light_count,
            version,
        }
    }
}

/// runtime 私有的 analytic light GPU buffer owner。
///
/// CPU scene 保存 point / spot / area light 语义；本 manager 对账只读快照并上传对应 FIF
/// buffer。变化会标记全部 FIF，保证每个 frame label
/// 在重新被使用前都能写入最新 light snapshot。
pub(crate) struct AnalyticLightTable {
    frames: [AnalyticLightFrameBuffers; FrameLabel::COUNT],
    fif_dirty: [bool; FrameLabel::COUNT],
    pending_commits: [bool; FrameLabel::COUNT],
    point_light_count: u32,
    spot_light_count: u32,
    area_light_count: u32,
    version: u32,
    /// 完整对账后保存的场景灯光副本；每个 FIF 上传只读取这些副本。
    point_lights: Vec<gpu::engine::light::PointLight>,
    spot_lights: Vec<gpu::engine::light::SpotLight>,
    area_lights: Vec<gpu::engine::light::AreaLight>,
}

impl AnalyticLightTable {
    pub(crate) fn new(resource_ctx: GfxResourceCtx<'_>) -> Self {
        Self {
            frames: FrameLabel::ALL.map(|label| AnalyticLightFrameBuffers::new(resource_ctx, label)),
            fif_dirty: [false; FrameLabel::COUNT],
            pending_commits: [false; FrameLabel::COUNT],
            point_light_count: 0,
            spot_light_count: 0,
            area_light_count: 0,
            version: 0,
            point_lights: Vec::new(),
            spot_lights: Vec::new(),
            area_lights: Vec::new(),
        }
    }

    pub(crate) fn mark_dirty(&mut self) {
        self.fif_dirty = [true; FrameLabel::COUNT];
    }

    /// 对账 CPU light revision；无需依赖 scene dirty event 才能发现新增或删除光源。
    pub(crate) fn sync_scene(&mut self, scene: SceneReadView<'_>) {
        if self.version != scene.light_revision() {
            self.point_lights = scene
                .point_light_map()
                .values()
                .map(|light| gpu::engine::light::PointLight {
                    pos: light.pos,
                    color: light.color,
                    _color_padding: Default::default(),
                    _pos_padding: Default::default(),
                })
                .collect();
            self.spot_lights = scene
                .spot_light_map()
                .values()
                .map(|light| gpu::engine::light::SpotLight {
                    pos: light.pos,
                    inner_angle: light.inner_angle,
                    color: light.color,
                    outer_angle: light.outer_angle,
                    dir: light.dir,
                    _dir_padding: Default::default(),
                })
                .collect();
            self.area_lights = scene
                .area_light_map()
                .values()
                .map(|light| gpu::engine::light::AreaLight {
                    center: light.center,
                    half_u: light.half_u,
                    half_v: light.half_v,
                    radiance: light.radiance,
                    _center_padding: Default::default(),
                    _half_u_padding: Default::default(),
                    _half_v_padding: Default::default(),
                    _radiance_padding: Default::default(),
                })
                .collect();
            self.point_light_count = u32::try_from(self.point_lights.len()).expect("analytic point light count exceeds u32 range");
            self.spot_light_count = u32::try_from(self.spot_lights.len()).expect("analytic spot light count exceeds u32 range");
            self.area_light_count = u32::try_from(self.area_lights.len()).expect("analytic area light count exceeds u32 range");
            self.version = scene.light_revision();
            self.mark_dirty();
        }
    }

    pub(crate) fn update_and_upload(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        frame_label: FrameLabel,
    ) -> AnalyticLightBinding {
        let frame_index = *frame_label;
        let changed = self.fif_dirty[frame_index];
        if changed {
            self.upload_current_frame(resource_ctx, cmd, barrier_mask, frame_index);
            self.pending_commits[frame_index] = true;
        }

        self.frames[frame_index].binding(
            self.point_light_count,
            self.spot_light_count,
            self.area_light_count,
            self.version,
        )
    }

    pub(crate) fn commit_submitted_frame(&mut self, frame_label: FrameLabel) {
        let index = *frame_label;
        if self.pending_commits[index] {
            self.fif_dirty[index] = false;
            self.pending_commits[index] = false;
        }
    }

    pub(crate) fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>) {
        for frame in &mut self.frames {
            frame.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        }
    }

    fn upload_current_frame(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        frame_index: usize,
    ) {
        let frame = &mut self.frames[frame_index];
        let label = FrameLabel::ALL[frame_index];
        frame.point.ensure_capacity(resource_ctx, self.point_lights.len(), &format!("point light-{label}"));
        frame.spot.ensure_capacity(resource_ctx, self.spot_lights.len(), &format!("spot light-{label}"));
        frame.area.ensure_capacity(resource_ctx, self.area_lights.len(), &format!("area light-{label}"));

        frame.point.upload(resource_ctx, cmd, barrier_mask, &self.point_lights);
        frame.spot.upload(resource_ctx, cmd, barrier_mask, &self.spot_lights);
        frame.area.upload(resource_ctx, cmd, barrier_mask, &self.area_lights);
    }
}
