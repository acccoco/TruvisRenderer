use ash::vk;

use truvis_gfx::commands::barrier::{GfxBarrierMask, GfxBufferBarrier};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_gfx::resources::buffer::GfxBuffer;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_shader_binding::gpu;
use truvis_world::SceneReadView;

const MAX_ANALYTIC_LIGHT_COUNT: usize = 512;

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

struct AnalyticLightFrameBuffers {
    point_light_buffer: GfxStructuredBuffer<gpu::engine::light::PointLight>,
    point_light_stage_buffer: GfxStructuredBuffer<gpu::engine::light::PointLight>,
    spot_light_buffer: GfxStructuredBuffer<gpu::engine::light::SpotLight>,
    spot_light_stage_buffer: GfxStructuredBuffer<gpu::engine::light::SpotLight>,
    area_light_buffer: GfxStructuredBuffer<gpu::engine::light::AreaLight>,
    area_light_stage_buffer: GfxStructuredBuffer<gpu::engine::light::AreaLight>,
}

impl AnalyticLightFrameBuffers {
    fn new(ctx: GfxResourceCtx<'_>, frame_label: FrameLabel) -> Self {
        Self {
            point_light_buffer: GfxStructuredBuffer::new_ssbo(
                ctx,
                MAX_ANALYTIC_LIGHT_COUNT,
                format!("point light buffer-{}", frame_label),
            ),
            point_light_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                ctx,
                MAX_ANALYTIC_LIGHT_COUNT,
                format!("point light stage buffer-{}", frame_label),
            ),
            spot_light_buffer: GfxStructuredBuffer::new_ssbo(
                ctx,
                MAX_ANALYTIC_LIGHT_COUNT,
                format!("spot light buffer-{}", frame_label),
            ),
            spot_light_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                ctx,
                MAX_ANALYTIC_LIGHT_COUNT,
                format!("spot light stage buffer-{}", frame_label),
            ),
            area_light_buffer: GfxStructuredBuffer::new_ssbo(
                ctx,
                MAX_ANALYTIC_LIGHT_COUNT,
                format!("area light buffer-{}", frame_label),
            ),
            area_light_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                ctx,
                MAX_ANALYTIC_LIGHT_COUNT,
                format!("area light stage buffer-{}", frame_label),
            ),
        }
    }

    fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>) {
        self.point_light_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.point_light_stage_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.spot_light_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.spot_light_stage_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.area_light_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        self.area_light_stage_buffer.destroy_mut(resource_ctx, DestroyReason::Shutdown);
    }

    fn binding(
        &self,
        point_light_count: u32,
        spot_light_count: u32,
        area_light_count: u32,
        version: u32,
    ) -> AnalyticLightBinding {
        AnalyticLightBinding {
            point_lights: self.point_light_buffer.device_address(),
            spot_lights: self.spot_light_buffer.device_address(),
            area_lights: self.area_light_buffer.device_address(),
            point_light_count,
            spot_light_count,
            area_light_count,
            version,
        }
    }
}

/// analytic light 阶段对外返回的结构化结果。
pub(crate) struct RenderAnalyticLightUpdateResult {
    pub(crate) binding: AnalyticLightBinding,
    pub(crate) changed: bool,
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
            frames: FrameLabel::ALL.map(|frame_label| AnalyticLightFrameBuffers::new(resource_ctx, frame_label)),
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
    ) -> RenderAnalyticLightUpdateResult {
        let frame_index = *frame_label;
        let changed = self.fif_dirty[frame_index];
        if changed {
            self.upload_current_frame(resource_ctx, cmd, barrier_mask, frame_index);
            self.pending_commits[frame_index] = true;
        }

        RenderAnalyticLightUpdateResult {
            binding: self.frames[frame_index].binding(
                self.point_light_count,
                self.spot_light_count,
                self.area_light_count,
                self.version,
            ),
            changed,
        }
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
            frame.destroy_mut(resource_ctx);
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
        let point_light_count = self.point_lights.len();
        let spot_light_count = self.spot_lights.len();
        let area_light_count = self.area_lights.len();

        if point_light_count > MAX_ANALYTIC_LIGHT_COUNT
            || spot_light_count > MAX_ANALYTIC_LIGHT_COUNT
            || area_light_count > MAX_ANALYTIC_LIGHT_COUNT
        {
            panic!("analytic light cnt can not be larger than buffer");
        }

        {
            let point_light_buffer_slices = frame.point_light_stage_buffer.mapped_slice();
            let spot_light_buffer_slices = frame.spot_light_stage_buffer.mapped_slice();
            let area_light_buffer_slices = frame.area_light_stage_buffer.mapped_slice();

            for (light_idx, point_light) in self.point_lights.iter().enumerate() {
                point_light_buffer_slices[light_idx] = *point_light;
            }

            for (light_idx, spot_light) in self.spot_lights.iter().enumerate() {
                spot_light_buffer_slices[light_idx] = *spot_light;
            }

            for (light_idx, area_light) in self.area_lights.iter().enumerate() {
                area_light_buffer_slices[light_idx] = *area_light;
            }
        }

        Self::flush_copy_and_barrier(
            resource_ctx,
            cmd,
            &mut frame.point_light_stage_buffer,
            &mut frame.point_light_buffer,
            barrier_mask,
        );
        Self::flush_copy_and_barrier(
            resource_ctx,
            cmd,
            &mut frame.spot_light_stage_buffer,
            &mut frame.spot_light_buffer,
            barrier_mask,
        );
        Self::flush_copy_and_barrier(
            resource_ctx,
            cmd,
            &mut frame.area_light_stage_buffer,
            &mut frame.area_light_buffer,
            barrier_mask,
        );

        debug_assert_eq!(point_light_count, self.point_light_count as usize);
        debug_assert_eq!(spot_light_count, self.spot_light_count as usize);
        debug_assert_eq!(area_light_count, self.area_light_count as usize);
    }

    fn flush_copy_and_barrier(
        resource_ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        stage_buffer: &mut GfxBuffer,
        dst: &mut GfxBuffer,
        barrier_mask: GfxBarrierMask,
    ) {
        let buffer_size = stage_buffer.size();
        stage_buffer.flush(resource_ctx, 0, buffer_size);
        cmd.cmd_copy_buffer(
            stage_buffer,
            dst,
            &[vk::BufferCopy {
                size: buffer_size,
                ..Default::default()
            }],
        );
        cmd.buffer_memory_barrier(
            vk::DependencyFlags::empty(),
            &[GfxBufferBarrier::default().mask(barrier_mask).buffer(dst.vk_buffer(), 0, vk::WHOLE_SIZE)],
        );
    }
}
