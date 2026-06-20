use crate::gui_plugin::{DebugImageEntry, DebugImageGraphEntry};
use crate::render_pipeline::common_settings::{PathTracingCommonSettings, RtSkySamplingMode};
use crate::render_pipeline::rt_render_graph::RtDebugChannel;
use crate::render_pipeline::targets::{ImageTarget, OfflineTargets};
use app_render_passes::accum_pass::{AccumPass, AccumRgPass};
use app_render_passes::image_clear_pass::{ImageClearPass, ImageClearRgPass};
use app_render_passes::offline_rt_pass::{OfflineRtPass, OfflineRtRgPass};
use app_render_passes::resolve_pass::{ResolvePass, ResolveRgPass};
use app_render_passes::sdr_pass::{SdrPass, SdrRgPass};
use std::cell::Cell;
use truvis_app_frame::plugin_api::{Plugin, PluginInitCtx, PluginRenderCtx, PluginResizeCtx, PluginShutdownCtx};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_foundation::frame_counter::{FrameCounter, FrameLabel};
use truvis_render_foundation::render_scene_view::RenderSceneAccumSignature;
use truvis_render_foundation::render_view::RenderViewAccumSignature;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle, RgImageState};

/// 离线 ground truth 管线。
///
/// 它和实时 `RtPipeline` 并列存在：资源、sample count 与 present input 都由本类型维护；
/// shader 侧只复用 path tracing 的 NEE/MIS helper，不绑定 DLSS 或 ReSTIR 资源。
#[derive(Default)]
pub struct OfflinePipeline {
    inner: Option<OfflinePipelineInner>,
    settings: OfflinePipelineSettings,
    accum_state: OfflineAccumState,
}

/// 离线管线自有设置。
///
/// 这些参数只影响离线 path tracing、累计和显示映射，不写入 runtime-owned DLSS / ReSTIR state。
#[derive(Clone, Copy)]
pub struct OfflinePipelineSettings {
    pub ray_dispatch_count: u32,
    pub debug_channel: RtDebugChannel,
}

impl Default for OfflinePipelineSettings {
    fn default() -> Self {
        Self {
            ray_dispatch_count: Self::MIN_RAY_DISPATCH_COUNT,
            debug_channel: RtDebugChannel::Final,
        }
    }
}

impl OfflinePipelineSettings {
    pub const MIN_RAY_DISPATCH_COUNT: u32 = 1;
    pub const MAX_RAY_DISPATCH_COUNT: u32 = 8;

    pub fn clamp_ray_dispatch_count(value: u32) -> u32 {
        value.clamp(Self::MIN_RAY_DISPATCH_COUNT, Self::MAX_RAY_DISPATCH_COUNT)
    }

    pub fn set_ray_dispatch_count(&mut self, value: u32) {
        self.ray_dispatch_count = Self::clamp_ray_dispatch_count(value);
    }

    pub fn effective_ray_dispatch_count(self) -> u32 {
        Self::clamp_ray_dispatch_count(self.ray_dispatch_count)
    }

    fn accum_signature(self, common_settings: &PathTracingCommonSettings) -> OfflineSettingsAccumSignature {
        // 每帧 dispatch 数只改变累计推进速度，不改变任一 sample 的 radiance 定义，
        // 因此不进入历史签名，避免用户调节吞吐量时重置 accum_image。
        // tone mapping 只影响 HDR -> SDR 显示映射，不改变 accum_image 中累计的 radiance。
        OfflineSettingsAccumSignature {
            debug_channel: self.debug_channel,
            sky_sampling_mode: common_settings.sky_sampling_mode,
            sky_brightness_bits: common_settings.sky_brightness.to_bits(),
            emissive_nee_enabled: common_settings.emissive_nee_enabled,
            analytic_nee_enabled: common_settings.analytic_nee_enabled,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OfflineSettingsAccumSignature {
    debug_channel: RtDebugChannel,
    sky_sampling_mode: RtSkySamplingMode,
    sky_brightness_bits: u32,
    emissive_nee_enabled: bool,
    analytic_nee_enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct OfflineAccumSignature {
    view: RenderViewAccumSignature,
    scene: RenderSceneAccumSignature,
    settings: OfflineSettingsAccumSignature,
}

#[derive(Clone, Copy, Debug)]
struct OfflineSampleState {
    /// 当前 sample-per-pixel 序号，作为 shader 随机种子的稳定输入。
    spp_idx: u32,
    /// 本次 sample 进入 accum pass 之前，`accum_image` 中已经融合的历史样本数。
    accum_frames: u32,
    /// 离线管线自有的 primary ray sub-pixel jitter，单位保持 pixel。
    sample_jitter_px: glam::Vec2,
}

/// 离线累计状态。
///
/// `sample_count` 是 `accum_image` 中已经融合的样本数量；它不复用 runtime 的
/// `ViewAccumState`，避免实时 DLSS/temporal reset 语义污染离线 reference 结果。
/// primary ray jitter 也由本类型按 sample index 生成，不能读取 realtime/DLSS jitter。
#[derive(Default)]
pub struct OfflineAccumState {
    sample_count: Cell<u32>,
    signature: Option<OfflineAccumSignature>,
}

impl OfflineAccumState {
    fn update_signature(&mut self, signature: OfflineAccumSignature) {
        if self.signature != Some(signature) {
            self.signature = Some(signature);
            self.reset();
        }
    }

    pub fn reset(&self) {
        self.sample_count.set(0);
    }

    fn take_next_sample_batch(&self, dispatch_count: u32) -> Vec<OfflineSampleState> {
        let dispatch_count = OfflinePipelineSettings::clamp_ray_dispatch_count(dispatch_count);
        // 只有确定本帧会提交 RT + accum pass 后才调用本函数。sample_count 在这里按 batch 推进，
        // 能避免 TLAS 暂不可用或 graph 被清理分支截断时把跳过帧计入离线累计。
        let accum_frames = self.sample_count.get();
        let next_sample_count = accum_frames.saturating_add(dispatch_count);
        let sample_states = (0..dispatch_count)
            .map(|dispatch_idx| {
                let sample_index = accum_frames.saturating_add(dispatch_idx);
                OfflineSampleState {
                    spp_idx: sample_index,
                    accum_frames: sample_index,
                    sample_jitter_px: Self::sample_jitter_px(sample_index.saturating_add(1)),
                }
            })
            .collect();
        self.sample_count.set(next_sample_count);
        // 低频记录离线累计推进，便于没有 GUI 自动化时从运行日志确认 `accum_image` 持续融合新样本。
        if next_sample_count >= 16 && next_sample_count.is_power_of_two() {
            log::info!("Offline accumulation sample_count={next_sample_count}");
        }
        sample_states
    }

    pub fn sample_count(&self) -> u32 {
        self.sample_count.get()
    }

    fn sample_jitter_px(sample_index: u32) -> glam::Vec2 {
        // Halton 序列使用 1-based sample index，避开 index 0 的全零点；返回值以 pixel
        // 为单位并围绕像素中心对称，shader 不需要再读取 realtime/DLSS temporal jitter。
        glam::vec2(Self::halton(sample_index, 2) - 0.5, Self::halton(sample_index, 3) - 0.5)
    }

    fn halton(mut index: u32, base: u32) -> f32 {
        let inv_base = 1.0 / base as f32;
        let mut fraction = inv_base;
        let mut value = 0.0;
        while index > 0 {
            value += (index % base) as f32 * fraction;
            index /= base;
            fraction *= inv_base;
        }
        value
    }
}

struct OfflinePipelineInner {
    offline_rt_pass: OfflineRtPass,
    image_clear_pass: ImageClearPass,
    accum_pass: AccumPass,
    sdr_pass: SdrPass,
    resolve_pass: ResolvePass,
    targets: OfflineTargets,
    compute_cmds: [GfxCommandBuffer; FrameCounter::fif_count()],
    present_cmds: [GfxCommandBuffer; FrameCounter::fif_count()],
}

pub struct OfflinePresentGraphTargets {
    pub present_image: RgImageHandle,
    pub offline_render_target: RgImageHandle,
}

impl OfflinePresentGraphTargets {
    pub fn debug_graph_entries(&self) -> [DebugImageGraphEntry; 1] {
        [DebugImageGraphEntry::new(
            "offline-render-target",
            self.offline_render_target,
            RgImageState::SHADER_READ_FRAGMENT,
        )]
    }
}

impl OfflinePipelineInner {
    fn new(ctx: &mut PluginInitCtx) -> Self {
        let offline_rt_pass = OfflineRtPass::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.device_info_ctx,
            ctx.shader_binding_system.global_descriptor_sets(),
        );
        let image_clear_pass = ImageClearPass::new(ctx.device_ctx, ctx.shader_binding_system.global_descriptor_sets());
        let accum_pass = AccumPass::new(ctx.device_ctx, ctx.shader_binding_system.global_descriptor_sets());
        let sdr_pass = SdrPass::new(ctx.device_ctx, ctx.shader_binding_system.global_descriptor_sets());
        let resolve_pass = ResolvePass::new(
            ctx.device_ctx,
            ctx.shader_binding_system.global_descriptor_sets(),
            ctx.present.swapchain_image_info().image_format,
        );

        let mut target_frame_state = *ctx.frame_state;
        target_frame_state.set_native_extent(ctx.swapchain_image_info.image_extent);
        let targets = OfflineTargets::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut *ctx.gfx_resource_manager,
            &mut *ctx.shader_binding_system,
            &target_frame_state,
            ctx.frame_timing.frame_counter(),
        );

        let compute_cmds = FrameCounter::frame_labes().map(|frame_label| {
            ctx.cmd_allocator.alloc_command_buffer(ctx.device_ctx, frame_label, "offline-compute-subgraph")
        });
        let present_cmds = FrameCounter::frame_labes().map(|frame_label| {
            ctx.cmd_allocator.alloc_command_buffer(ctx.device_ctx, frame_label, "offline-present-subgraph")
        });

        Self {
            offline_rt_pass,
            image_clear_pass,
            accum_pass,
            sdr_pass,
            resolve_pass,
            targets,
            compute_cmds,
            present_cmds,
        }
    }

    fn destroy(mut self, ctx: &mut PluginShutdownCtx<'_>) {
        self.offline_rt_pass.destroy(ctx.resource_ctx, ctx.device_ctx);
        self.image_clear_pass.destroy(ctx.device_ctx);
        self.accum_pass.destroy(ctx.device_ctx);
        self.sdr_pass.destroy(ctx.device_ctx);
        self.resolve_pass.destroy(ctx.device_ctx);
        self.targets.destroy(
            ctx.resource_ctx,
            ctx.device_ctx,
            &mut *ctx.shader_binding_system,
            &mut *ctx.gfx_resource_manager,
            DestroyReason::Shutdown,
        );
    }
}

impl Plugin for OfflinePipeline {
    fn init(&mut self, ctx: &mut PluginInitCtx) {
        self.inner = Some(OfflinePipelineInner::new(ctx));
    }

    fn on_resize(&mut self, ctx: &mut PluginResizeCtx) {
        if let Some(inner) = self.inner.as_mut() {
            let mut target_frame_state = *ctx.frame_state;
            target_frame_state.set_native_extent(ctx.present.swapchain_image_info().image_extent);
            inner.targets.rebuild(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.immediate_ctx,
                &mut *ctx.shader_binding_system,
                &mut *ctx.gfx_resource_manager,
                &target_frame_state,
                ctx.frame_timing.frame_counter(),
            );
            self.accum_state.reset();
        }
    }

    fn shutdown(&mut self, ctx: &mut PluginShutdownCtx<'_>) {
        if let Some(inner) = self.inner.take() {
            inner.destroy(ctx);
        }
    }
}

impl OfflinePipeline {
    pub fn settings(&self) -> &OfflinePipelineSettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut OfflinePipelineSettings {
        &mut self.settings
    }

    pub fn sample_count(&self) -> u32 {
        self.accum_state.sample_count()
    }

    pub fn update_accum_signature(
        &mut self,
        view_signature: RenderViewAccumSignature,
        scene_signature: RenderSceneAccumSignature,
        common_settings: &PathTracingCommonSettings,
    ) {
        // 调用方必须在本帧相机、scene root、TLAS、light 和 sky 数据确定后更新签名。
        // 这里仅比较“是否还能复用 accum_image 历史”，不主动读取 runtime 的 ViewAccumState。
        self.accum_state.update_signature(OfflineAccumSignature {
            view: view_signature,
            scene: scene_signature,
            settings: self.settings.accum_signature(common_settings),
        });
    }

    pub fn compute_cmd(&self, frame_label: FrameLabel) -> &GfxCommandBuffer {
        &self.inner().compute_cmds[*frame_label]
    }

    pub fn present_cmd(&self, frame_label: FrameLabel) -> &GfxCommandBuffer {
        &self.inner().present_cmds[*frame_label]
    }

    pub fn contribute_compute_passes<'a>(
        &'a self,
        rg_builder: &mut RenderGraphBuilder<'a>,
        ctx: &'a PluginRenderCtx<'a>,
        common_settings: &PathTracingCommonSettings,
    ) {
        let inner = self.inner();
        let record_ctx = ctx.record_ctx;
        let frame_label = record_ctx.frame_timing.frame_label();
        // TLAS readiness 在 pipeline 层提前判断：没有 TLAS 时不创建 RT/accum pass，
        // 也不推进 OfflineAccumState，保证空场景或上传暂缺不会污染 sample count。
        let has_tlas = ctx.render_scene.tlas_handle(frame_label).is_some();
        let targets = &inner.targets;
        let debug_channel = self.settings.debug_channel.shader_channel();
        let sky_sampling_mode = common_settings.sky_sampling_mode.shader_mode();
        let sky_brightness = common_settings.sky_brightness;
        let emissive_nee_enabled = common_settings.emissive_nee_enabled;
        let analytic_nee_enabled = common_settings.analytic_nee_enabled;
        let tone_mapping = common_settings.tone_mapping;

        let single_frame_target = targets.single_frame_image(frame_label);
        let single_frame_image = rg_builder.import_image(
            "offline-single-frame",
            single_frame_target.image,
            Some(single_frame_target.view),
            single_frame_target.format,
            RgImageState::UNDEFINED_TOP,
            None,
        );

        let accum_target = targets.accum_image();
        let accum_image = rg_builder.import_image(
            "offline-accum",
            accum_target.image,
            Some(accum_target.view),
            accum_target.format,
            // 有 TLAS 时保留并读写跨帧历史；无 TLAS 时后续 clear 会完整覆盖，import 可从未定义状态开始。
            if has_tlas { RgImageState::STORAGE_READ_WRITE_COMPUTE } else { RgImageState::UNDEFINED_TOP },
            None,
        );

        let render_target_info = targets.render_target(frame_label);
        let render_target = rg_builder.import_image(
            "offline-render-target",
            render_target_info.image,
            Some(render_target_info.view),
            render_target_info.format,
            RgImageState::UNDEFINED_TOP,
            None,
        );
        rg_builder.export_image(render_target, RgImageState::SHADER_READ_FRAGMENT, None);

        if !has_tlas {
            // 无 TLAS 是合法的“当前没有可追踪场景”状态。这里输出确定黑色并重置离线历史，
            // 避免 GUI/present target 继续展示上一帧场景或未定义 image 内容。
            self.accum_state.reset();
            let clear_color = glam::Vec4::ZERO;
            rg_builder
                .add_pass(
                    "offline-clear-single-frame",
                    ImageClearRgPass {
                        clear_pass: &inner.image_clear_pass,
                        record_ctx,
                        dst_image: single_frame_image,
                        image_extent: single_frame_target.extent,
                        clear_color,
                    },
                )
                .add_pass(
                    "offline-clear-accum",
                    ImageClearRgPass {
                        clear_pass: &inner.image_clear_pass,
                        record_ctx,
                        dst_image: accum_image,
                        image_extent: accum_target.extent,
                        clear_color,
                    },
                )
                .add_pass(
                    "offline-clear-render-target",
                    ImageClearRgPass {
                        clear_pass: &inner.image_clear_pass,
                        record_ctx,
                        dst_image: render_target,
                        image_extent: render_target_info.extent,
                        clear_color,
                    },
                );
            return;
        }

        let sample_states = self.accum_state.take_next_sample_batch(self.settings.effective_ray_dispatch_count());
        // 每个 dispatch 都覆盖同一张 single_frame_image，并立即累入同一张 accum_image。
        // RenderGraph 的线性 pass 顺序负责在相邻 RT/accum pair 之间插入必要的 image 状态转换。
        for (dispatch_idx, sample_state) in sample_states.into_iter().enumerate() {
            let dispatch_ordinal = dispatch_idx + 1;
            rg_builder
                .add_pass(
                    format!("offline-ray-tracing-{dispatch_ordinal}"),
                    OfflineRtRgPass {
                        rt_pass: &inner.offline_rt_pass,
                        record_ctx,
                        render_scene: ctx.render_scene,
                        single_frame_image,
                        single_frame_extent: single_frame_target.extent,
                        spp_idx: sample_state.spp_idx,
                        sample_jitter_px: sample_state.sample_jitter_px,
                        debug_channel,
                        sky_sampling_mode,
                        sky_brightness,
                        emissive_nee_enabled,
                        analytic_nee_enabled,
                    },
                )
                .add_pass(
                    format!("offline-accum-{dispatch_ordinal}"),
                    AccumRgPass {
                        accum_pass: &inner.accum_pass,
                        record_ctx,
                        single_frame_image,
                        accum_image,
                        image_extent: accum_target.extent,
                        accum_frames: sample_state.accum_frames,
                    },
                );
        }
        rg_builder.add_pass(
            "offline-hdr-to-sdr",
            SdrRgPass {
                sdr_pass: &inner.sdr_pass,
                record_ctx,
                src_image: accum_image,
                dst_image: render_target,
                src_image_extent: accum_target.extent,
                dst_image_extent: render_target_info.extent,
                debug_channel,
                tone_mapping,
            },
        );
    }

    pub fn contribute_present_passes<'a>(
        &'a self,
        rg_builder: &mut RenderGraphBuilder<'a>,
        ctx: &'a PluginRenderCtx<'a>,
        _common_settings: &PathTracingCommonSettings,
    ) -> OfflinePresentGraphTargets {
        let inner = self.inner();
        let record_ctx = ctx.record_ctx;
        let frame_label = record_ctx.frame_timing.frame_label();
        let color_target = inner.targets.render_target(frame_label);
        let render_target = rg_builder.import_image(
            "offline-render-target",
            color_target.image,
            Some(color_target.view),
            color_target.format,
            RgImageState::SHADER_READ_FRAGMENT,
            None,
        );

        let present_target = ctx.present.import_current_target(rg_builder, frame_label);
        let present_image = present_target.image;

        rg_builder.add_pass(
            "resolve",
            ResolveRgPass {
                resolve_pass: &inner.resolve_pass,
                record_ctx,
                render_target,
                swapchain_image: present_image,
                swapchain_extent: present_target.image_info.image_extent,
            },
        );

        OfflinePresentGraphTargets {
            present_image,
            offline_render_target: render_target,
        }
    }

    pub fn collect_debug_images(&self, frame_label: FrameLabel) -> Vec<DebugImageEntry> {
        let targets = &self.inner().targets;
        vec![
            debug_entry("offline-single-frame", "Offline Single Frame", targets.single_frame_image(frame_label)),
            debug_entry("offline-accum", "Offline Accum", targets.accum_image()),
            debug_entry("offline-render-target", "Offline Render Target", targets.render_target(frame_label)),
        ]
    }

    fn inner(&self) -> &OfflinePipelineInner {
        self.inner.as_ref().expect("OfflinePipeline not initialized")
    }
}

fn debug_entry(id: &'static str, label: &'static str, target: ImageTarget) -> DebugImageEntry {
    DebugImageEntry::raw(id, label, target.image, target.view, target.format, target.extent)
}

impl Drop for OfflinePipeline {
    fn drop(&mut self) {
        log::info!("OfflinePipeline drop");
    }
}
