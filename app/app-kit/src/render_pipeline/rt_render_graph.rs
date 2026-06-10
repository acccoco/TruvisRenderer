use crate::gui_plugin::{DebugImageEntry, DebugImageGraphEntry};
use crate::render_pipeline::targets::{
    DlssRrInputTargets, DlssSrInputTargets, DlssSrOutputTargets, ImageTarget, MainViewTargets, RtWorkingTargets,
};
use app_render_passes::dlss_rr_pass::{DlssRrPass, DlssRrRgPass};
use app_render_passes::dlss_sr_pass::{DLSS_SR_INPUT_READ, DlssSrPass, DlssSrRgPass};
use app_render_passes::gbuffer::GBuffer;
use app_render_passes::realtime_rt_pass::{RealtimeRtPass, RealtimeRtRgPass};
use app_render_passes::resolve_pass::{ResolvePass, ResolveRgPass};
use app_render_passes::sdr_pass::{SdrPass, SdrRgPass, SdrToneMappingSettings};
use truvis_app_frame::plugin_api::{Plugin, PluginInitCtx, PluginRenderCtx, PluginResizeCtx, PluginShutdownCtx};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_foundation::frame_counter::{FrameCounter, FrameLabel};
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle, RgImageState};
use truvis_render_runtime::state::dlss_sr::DlssSrMode;

#[derive(Default)]
pub struct RtPipeline {
    inner: Option<RtPipelineInner>,
    /// RT app 自有的可调参数。
    ///
    /// 生命周期跟随 `RtPipeline`，由 Truvis / Cornell 等 RT app 在 ImGui update 阶段修改，
    /// 再在构建 render graph 时显式传给相关 pass。
    settings: RtPipelineSettings,
}

/// RT pipeline 自有配置。
///
/// 这些选项只影响 app 层 RT pass 和后处理调试输出，不进入 engine runtime-owned render state。
#[derive(Clone, Copy)]
pub struct RtPipelineSettings {
    /// 当前 RT 调试输出通道。
    ///
    /// 这是 RT 主流程的 pass-local 配置，不影响 engine runtime 的 target 尺寸、DLSS history
    /// 或全局 per-frame UBO，因此不放入 engine runtime-owned render state。
    pub debug_channel: RtDebugChannel,
    /// SDR 输出路径的 tone mapping 参数。
    ///
    /// 只影响 Final 通道的 `hdr-to-sdr` pass，不改变 render extent、DLSS feature resource
    /// 或 runtime-owned temporal state。
    pub tone_mapping: SdrToneMappingSettings,
}

impl Default for RtPipelineSettings {
    fn default() -> Self {
        Self {
            debug_channel: RtDebugChannel::Final,
            tone_mapping: SdrToneMappingSettings::default(),
        }
    }
}

/// 主 RT 流程支持的调试通道。
///
/// 数值由 RT/Sdr shader push constant 消费；这里用 enum 固定语义，避免 UI 直接暴露 magic number。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtDebugChannel {
    /// 标准最终颜色输出。
    Final,
    /// 显示命中点法线，用于检查几何与 shading normal。
    Normal,
    /// 显示材质 base color / albedo。
    BaseColor,
    /// 显示 next-event estimation 中来自 HDRI 的直接光。
    NeeHdri,
    /// 显示自发光材质贡献。
    Emission,
    /// 显示 BRDF 采样到 HDRI 的间接贡献。
    BrdfHdri,
    /// 显示第 0 次 bounce 的 NEE 贡献。
    NeeBounce0,
    /// 显示第 1 次 bounce 的 NEE 贡献。
    NeeBounce1,
    /// 显示 shader 实验路径中的 Irradiance Cache 调试结果。
    ///
    /// 当前主流程固定 `ic_enabled = 0`，因此该通道只是保留 shader 侧观测入口，
    /// 不代表 engine 提供了全局 IC 配置。
    IrradianceCache,
}

impl RtDebugChannel {
    pub const ALL: [Self; 9] = [
        Self::Final,
        Self::Normal,
        Self::BaseColor,
        Self::NeeHdri,
        Self::Emission,
        Self::BrdfHdri,
        Self::NeeBounce0,
        Self::NeeBounce1,
        Self::IrradianceCache,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Final => "final",
            Self::Normal => "normal",
            Self::BaseColor => "base color",
            Self::NeeHdri => "from NEE HDRI",
            Self::Emission => "from emission",
            Self::BrdfHdri => "from BRDF HDRI",
            Self::NeeBounce0 => "NEE bounce 0",
            Self::NeeBounce1 => "NEE bounce 1",
            Self::IrradianceCache => "Irradiance Cache",
        }
    }

    pub fn shader_channel(self) -> u32 {
        match self {
            Self::Final => 0,
            Self::Normal => 1,
            Self::BaseColor => 2,
            Self::NeeHdri => 4,
            Self::Emission => 5,
            Self::BrdfHdri => 6,
            Self::NeeBounce0 => 7,
            Self::NeeBounce1 => 8,
            Self::IrradianceCache => 9,
        }
    }
}

struct RtPipelineInner {
    realtime_rt_pass: RealtimeRtPass,
    /// DLSS SR 是外部 opaque pass，不拥有 shader pipeline；只在 SR/DLAA 分支被加入 compute graph。
    dlss_sr_pass: DlssSrPass,
    /// DLSS RR 是 SR 基础设施上的替代 evaluate 分支，不与 `dlss_sr_pass` 连续运行。
    dlss_rr_pass: DlssRrPass,
    sdr_pass: SdrPass,
    resolve_pass: ResolvePass,
    gbuffer: GBuffer,
    /// RT 私有工作图像。它们的格式/用途由 RT pipeline 决定，因此不再放在 engine runtime state。
    rt_targets: RtWorkingTargets,
    /// DLSS SR 需要的低分辨率 depth/motion-vector 输入。
    ///
    /// 即使 SR 关闭也会由 raygen 写入，便于 ImGui debug viewer 验证深度和 motion vector。
    dlss_sr_inputs: DlssSrInputTargets,
    /// DLSS RR 额外需要的低分辨率输入。
    dlss_rr_inputs: DlssRrInputTargets,
    /// DLSS SR 输出的高分辨率 HDR color。
    dlss_sr_outputs: DlssSrOutputTargets,
    /// 主视图离屏目标。compute graph 写入 color，present graph 再 resolve 到 swapchain。
    main_view_targets: MainViewTargets,
    compute_cmds: [GfxCommandBuffer; FrameCounter::fif_count()],
    present_cmds: [GfxCommandBuffer; FrameCounter::fif_count()],
}

/// RT present graph 中已经导入的关键图像。
///
/// 调用方把 `present_image` 交给 GUI 叠加；`main_view_color` 可作为 debug image 复用，
/// 避免同一物理图像在 present graph 内重复 import。
pub struct RtPresentGraphTargets {
    pub present_image: RgImageHandle,
    pub main_view_color: RgImageHandle,
}

impl RtPresentGraphTargets {
    pub fn debug_graph_entries(&self) -> [DebugImageGraphEntry; 1] {
        [DebugImageGraphEntry::new(
            "main-view-color",
            self.main_view_color,
            RgImageState::SHADER_READ_FRAGMENT,
        )]
    }
}

impl RtPipelineInner {
    fn new(ctx: &mut PluginInitCtx) -> Self {
        let realtime_rt_pass = RealtimeRtPass::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.device_info_ctx,
            ctx.immediate_ctx,
            ctx.shader_binding_system.global_descriptor_sets(),
        );
        let dlss_sr_pass = DlssSrPass::new();
        let dlss_rr_pass = DlssRrPass::new();
        let sdr_pass = SdrPass::new(ctx.device_ctx, ctx.shader_binding_system.global_descriptor_sets());
        let resolve_pass = ResolvePass::new(
            ctx.device_ctx,
            ctx.shader_binding_system.global_descriptor_sets(),
            ctx.present.swapchain_image_info().image_format,
        );
        // `RenderRuntime::new` 早于窗口创建，只能给 `FrameRenderState` 一个占位 extent；
        // app-owned target 必须使用 init 阶段已经创建好的 swapchain extent，避免首帧按 400x400
        // 创建中间图像。runtime 会在 `init_after_window` 同步该值，这里仍显式覆盖，保证契约局部可见。
        let mut target_frame_state = *ctx.frame_state;
        target_frame_state.set_native_extent(ctx.swapchain_image_info.image_extent);

        let rt_targets = RtWorkingTargets::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut *ctx.gfx_resource_manager,
            &mut *ctx.shader_binding_system,
            &target_frame_state,
            ctx.frame_timing.frame_counter(),
        );
        let main_view_targets = MainViewTargets::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut *ctx.gfx_resource_manager,
            &mut *ctx.shader_binding_system,
            &target_frame_state,
            ctx.frame_timing.frame_counter(),
        );
        let dlss_sr_inputs = DlssSrInputTargets::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut *ctx.gfx_resource_manager,
            &mut *ctx.shader_binding_system,
            &target_frame_state,
            ctx.frame_timing.frame_counter(),
        );
        let dlss_rr_inputs = DlssRrInputTargets::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut *ctx.gfx_resource_manager,
            &mut *ctx.shader_binding_system,
            &target_frame_state,
            ctx.frame_timing.frame_counter(),
        );
        let dlss_sr_outputs = DlssSrOutputTargets::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut *ctx.gfx_resource_manager,
            &mut *ctx.shader_binding_system,
            &target_frame_state,
            ctx.frame_timing.frame_counter(),
        );

        let gbuffer = GBuffer::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut *ctx.gfx_resource_manager,
            &mut *ctx.shader_binding_system,
            target_frame_state.render_extent,
            ctx.frame_timing.frame_counter(),
        );

        let compute_cmds = FrameCounter::frame_labes().map(|frame_label| {
            ctx.cmd_allocator.alloc_command_buffer(ctx.device_ctx, frame_label, "rt-compute-subgraph")
        });
        let present_cmds = FrameCounter::frame_labes().map(|frame_label| {
            ctx.cmd_allocator.alloc_command_buffer(ctx.device_ctx, frame_label, "rt-present-subgraph")
        });

        Self {
            realtime_rt_pass,
            dlss_sr_pass,
            dlss_rr_pass,
            sdr_pass,
            resolve_pass,
            gbuffer,
            rt_targets,
            dlss_sr_inputs,
            dlss_rr_inputs,
            dlss_sr_outputs,
            main_view_targets,
            compute_cmds,
            present_cmds,
        }
    }

    fn destroy(mut self, ctx: &mut PluginShutdownCtx<'_>) {
        // pass pipeline 本身只依赖 device；target image/view 依赖 resource manager 和 bindless。
        // shutdown 阶段 runtime 已经 wait idle，先销毁 pipeline 再释放 target 不会影响 GPU 引用安全，
        // 但 target 仍必须在 runtime `GfxResourceManager` 销毁前显式释放。
        self.realtime_rt_pass.destroy(ctx.resource_ctx, ctx.device_ctx);
        self.dlss_sr_pass.destroy();
        self.dlss_rr_pass.destroy();
        self.sdr_pass.destroy(ctx.device_ctx);
        self.resolve_pass.destroy(ctx.device_ctx);
        self.gbuffer.destroy(
            ctx.resource_ctx,
            ctx.device_ctx,
            &mut *ctx.shader_binding_system,
            &mut *ctx.gfx_resource_manager,
            DestroyReason::Shutdown,
        );
        self.rt_targets.destroy(
            ctx.resource_ctx,
            ctx.device_ctx,
            &mut *ctx.shader_binding_system,
            &mut *ctx.gfx_resource_manager,
            DestroyReason::Shutdown,
        );
        self.dlss_sr_inputs.destroy(
            ctx.resource_ctx,
            ctx.device_ctx,
            &mut *ctx.shader_binding_system,
            &mut *ctx.gfx_resource_manager,
            DestroyReason::Shutdown,
        );
        self.dlss_rr_inputs.destroy(
            ctx.resource_ctx,
            ctx.device_ctx,
            &mut *ctx.shader_binding_system,
            &mut *ctx.gfx_resource_manager,
            DestroyReason::Shutdown,
        );
        self.dlss_sr_outputs.destroy(
            ctx.resource_ctx,
            ctx.device_ctx,
            &mut *ctx.shader_binding_system,
            &mut *ctx.gfx_resource_manager,
            DestroyReason::Shutdown,
        );
        self.main_view_targets.destroy(
            ctx.resource_ctx,
            ctx.device_ctx,
            &mut *ctx.shader_binding_system,
            &mut *ctx.gfx_resource_manager,
            DestroyReason::Shutdown,
        );
    }
}

impl Plugin for RtPipeline {
    fn init(&mut self, ctx: &mut PluginInitCtx) {
        self.inner = Some(RtPipelineInner::new(ctx));
    }

    fn on_resize(&mut self, ctx: &mut PluginResizeCtx) {
        if let Some(inner) = self.inner.as_mut() {
            // resize ctx 来自 present 层实际重建后的安全点；旧 target 不会再被在飞命令引用。
            // 这里用 `PresentView` 再读一次 swapchain extent，避免 app-owned target 和
            // swapchain 在平台裁剪尺寸时出现细微不一致。
            let target_frame_state = *ctx.frame_state;
            inner.rt_targets.rebuild(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.immediate_ctx,
                &mut *ctx.shader_binding_system,
                &mut *ctx.gfx_resource_manager,
                &target_frame_state,
                ctx.frame_timing.frame_counter(),
            );
            inner.dlss_sr_inputs.rebuild(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.immediate_ctx,
                &mut *ctx.shader_binding_system,
                &mut *ctx.gfx_resource_manager,
                &target_frame_state,
                ctx.frame_timing.frame_counter(),
            );
            inner.dlss_rr_inputs.rebuild(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.immediate_ctx,
                &mut *ctx.shader_binding_system,
                &mut *ctx.gfx_resource_manager,
                &target_frame_state,
                ctx.frame_timing.frame_counter(),
            );
            inner.dlss_sr_outputs.rebuild(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.immediate_ctx,
                &mut *ctx.shader_binding_system,
                &mut *ctx.gfx_resource_manager,
                &target_frame_state,
                ctx.frame_timing.frame_counter(),
            );
            inner.main_view_targets.rebuild(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.immediate_ctx,
                &mut *ctx.shader_binding_system,
                &mut *ctx.gfx_resource_manager,
                &target_frame_state,
                ctx.frame_timing.frame_counter(),
            );
            inner.gbuffer.rebuild(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.immediate_ctx,
                &mut *ctx.shader_binding_system,
                &mut *ctx.gfx_resource_manager,
                target_frame_state.render_extent,
                ctx.frame_timing.frame_counter(),
            );
        }
    }

    fn shutdown(&mut self, ctx: &mut PluginShutdownCtx<'_>) {
        if let Some(inner) = self.inner.take() {
            inner.destroy(ctx);
        }
    }
}

impl RtPipeline {
    pub fn settings(&self) -> &RtPipelineSettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut RtPipelineSettings {
        &mut self.settings
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
    ) {
        let inner = self.inner();
        let record_ctx = ctx.record_ctx;
        let frame_label = record_ctx.frame_timing.frame_label();
        let rt_targets = &inner.rt_targets;
        let dlss_sr_inputs = &inner.dlss_sr_inputs;
        let dlss_rr_inputs = &inner.dlss_rr_inputs;
        let dlss_sr_outputs = &inner.dlss_sr_outputs;
        let main_view_targets = &inner.main_view_targets;
        let debug_channel = self.settings.debug_channel.shader_channel();
        let tone_mapping = self.settings.tone_mapping;

        // compute graph 导入的是 app-owned 外部图像；RenderGraph 只接管本图内的状态转换，
        // 不拥有图像生命周期。owner 必须活到 graph 录制与提交完成之后。
        let single_frame_target = rt_targets.single_frame_rt(frame_label);
        let single_frame_image = rg_builder.import_image(
            "single-frame-image",
            single_frame_target.image,
            Some(single_frame_target.view),
            single_frame_target.format,
            RgImageState::UNDEFINED_TOP,
            None,
        );

        let gbuffer = &inner.gbuffer;
        let (gbuffer_a_image_handle, gbuffer_a_view_handle) = gbuffer.a_handle(frame_label);
        let gbuffer_a = rg_builder.import_image(
            "gbuffer-a",
            gbuffer_a_image_handle,
            Some(gbuffer_a_view_handle),
            GBuffer::A_FORMAT,
            RgImageState::UNDEFINED_TOP,
            None,
        );

        let (gbuffer_b_image_handle, gbuffer_b_view_handle) = gbuffer.b_handle(frame_label);
        let gbuffer_b = rg_builder.import_image(
            "gbuffer-b",
            gbuffer_b_image_handle,
            Some(gbuffer_b_view_handle),
            GBuffer::B_FORMAT,
            RgImageState::UNDEFINED_TOP,
            None,
        );

        let (gbuffer_c_image_handle, gbuffer_c_view_handle) = gbuffer.c_handle(frame_label);
        let gbuffer_c = rg_builder.import_image(
            "gbuffer-c",
            gbuffer_c_image_handle,
            Some(gbuffer_c_view_handle),
            GBuffer::C_FORMAT,
            RgImageState::UNDEFINED_TOP,
            None,
        );

        let depth_target = dlss_sr_inputs.depth(frame_label);
        // depth/mvec 是 ray-tracing pass 写出的 per-frame SR 输入。初始状态从 UNDEFINED_TOP
        // 进入 graph，由 ray-tracing write 和后续 SR/debug read 决定精确 layout。
        let depth = rg_builder.import_image(
            "dlss-depth",
            depth_target.image,
            Some(depth_target.view),
            depth_target.format,
            RgImageState::UNDEFINED_TOP,
            None,
        );

        let motion_vectors_target = dlss_sr_inputs.motion_vectors(frame_label);
        let motion_vectors = rg_builder.import_image(
            "dlss-motion-vectors",
            motion_vectors_target.image,
            Some(motion_vectors_target.view),
            motion_vectors_target.format,
            RgImageState::UNDEFINED_TOP,
            None,
        );

        let rr_diffuse_albedo_target = dlss_rr_inputs.diffuse_albedo(frame_label);
        let rr_diffuse_albedo = rg_builder.import_image(
            "dlss-rr-diffuse-albedo",
            rr_diffuse_albedo_target.image,
            Some(rr_diffuse_albedo_target.view),
            rr_diffuse_albedo_target.format,
            RgImageState::UNDEFINED_TOP,
            None,
        );

        let rr_specular_albedo_target = dlss_rr_inputs.specular_albedo(frame_label);
        let rr_specular_albedo = rg_builder.import_image(
            "dlss-rr-specular-albedo",
            rr_specular_albedo_target.image,
            Some(rr_specular_albedo_target.view),
            rr_specular_albedo_target.format,
            RgImageState::UNDEFINED_TOP,
            None,
        );

        let rr_specular_motion_vectors_target = dlss_rr_inputs.specular_motion_vectors(frame_label);
        let rr_specular_motion_vectors = rg_builder.import_image(
            "dlss-rr-specular-motion-vectors",
            rr_specular_motion_vectors_target.image,
            Some(rr_specular_motion_vectors_target.view),
            rr_specular_motion_vectors_target.format,
            RgImageState::UNDEFINED_TOP,
            None,
        );

        let dlss_output_target = dlss_sr_outputs.color(frame_label);
        let dlss_output = rg_builder.import_image(
            "dlss-sr-output",
            dlss_output_target.image,
            Some(dlss_output_target.view),
            dlss_output_target.format,
            RgImageState::UNDEFINED_TOP,
            None,
        );

        // compute graph 的输出 target 会被 present graph 继续读取；导出状态固定为 fragment read，
        // 让后续 resolve/GUI 叠加路径以明确状态重新导入。
        let color_target = main_view_targets.color(frame_label);
        let render_target = rg_builder.import_image(
            "render-target",
            color_target.image,
            Some(color_target.view),
            color_target.format,
            RgImageState::UNDEFINED_TOP,
            None,
        );

        rg_builder.export_image(render_target, RgImageState::SHADER_READ_FRAGMENT, None);

        rg_builder.add_pass(
            "ray-tracing",
            RealtimeRtRgPass {
                rt_pass: &inner.realtime_rt_pass,
                record_ctx,
                render_scene: ctx.render_scene,
                single_frame_image,
                single_frame_extent: record_ctx.frame_state.render_extent,
                debug_channel,
                gbuffer_a,
                gbuffer_b,
                gbuffer_c,
                depth,
                motion_vectors,
                rr_diffuse_albedo,
                rr_specular_albedo,
                rr_specular_motion_vectors,
            },
        );

        if dlss_rr_active(record_ctx.render_options.dlss_sr_mode, record_ctx.render_options.dlss_rr_enabled) {
            rg_builder
                .add_pass(
                    "dlss-rr",
                    DlssRrRgPass {
                        dlss_rr_pass: &inner.dlss_rr_pass,
                        record_ctx,
                        resource_ctx: ctx.resource_ctx,
                        input_color: single_frame_image,
                        output_color: dlss_output,
                        depth,
                        motion_vectors,
                        diffuse_albedo: rr_diffuse_albedo,
                        specular_albedo: rr_specular_albedo,
                        normal_roughness: gbuffer_a,
                        specular_motion_vectors: rr_specular_motion_vectors,
                    },
                )
                .add_pass(
                    "hdr-to-sdr",
                    SdrRgPass {
                        sdr_pass: &inner.sdr_pass,
                        record_ctx,
                        src_image: dlss_output,
                        dst_image: render_target,
                        src_image_extent: record_ctx.frame_state.output_extent,
                        dst_image_extent: record_ctx.frame_state.output_extent,
                        debug_channel,
                        tone_mapping,
                    },
                );
        } else if dlss_sr_enabled(record_ctx.render_options.dlss_sr_mode) {
            // SR/DLAA 分支用 Streamline output 进入 SDR；不再运行传统 denoise/accum，
            // 也不在 SR 后追加第二个 upscale pass。
            rg_builder
                .add_pass(
                    "dlss-sr",
                    DlssSrRgPass {
                        dlss_sr_pass: &inner.dlss_sr_pass,
                        record_ctx,
                        resource_ctx: ctx.resource_ctx,
                        input_color: single_frame_image,
                        output_color: dlss_output,
                        depth,
                        motion_vectors,
                    },
                )
                .add_pass(
                    "hdr-to-sdr",
                    SdrRgPass {
                        sdr_pass: &inner.sdr_pass,
                        record_ctx,
                        src_image: dlss_output,
                        dst_image: render_target,
                        src_image_extent: record_ctx.frame_state.output_extent,
                        dst_image_extent: record_ctx.frame_state.output_extent,
                        debug_channel,
                        tone_mapping,
                    },
                );
        } else {
            // Native fallback 直接把低分辨率/原生 RT color 送入 SDR。此时 render/output extent
            // 通常相等；若未来支持非 DLSS upscale，这里需要重新明确尺寸契约。
            rg_builder.add_pass(
                "hdr-to-sdr",
                SdrRgPass {
                    sdr_pass: &inner.sdr_pass,
                    record_ctx,
                    src_image: single_frame_image,
                    dst_image: render_target,
                    src_image_extent: record_ctx.frame_state.render_extent,
                    dst_image_extent: record_ctx.frame_state.output_extent,
                    debug_channel,
                    tone_mapping,
                },
            );
        }
    }

    pub fn collect_debug_images(
        &self,
        frame_label: FrameLabel,
        dlss_sr_mode: DlssSrMode,
        dlss_rr_enabled: bool,
    ) -> Vec<DebugImageEntry> {
        let inner = self.inner();
        let rt_targets = &inner.rt_targets;
        let main_view_targets = &inner.main_view_targets;
        let dlss_sr_inputs = &inner.dlss_sr_inputs;
        let dlss_rr_inputs = &inner.dlss_rr_inputs;
        let dlss_sr_outputs = &inner.dlss_sr_outputs;
        let gbuffer = &inner.gbuffer;

        let single_frame = rt_targets.single_frame_rt(frame_label);
        let main_view_color = main_view_targets.color(frame_label);
        let depth = dlss_sr_inputs.depth(frame_label);
        let motion_vectors = dlss_sr_inputs.motion_vectors(frame_label);
        let rr_diffuse_albedo = dlss_rr_inputs.diffuse_albedo(frame_label);
        let rr_specular_albedo = dlss_rr_inputs.specular_albedo(frame_label);
        let rr_specular_motion_vectors = dlss_rr_inputs.specular_motion_vectors(frame_label);
        let dlss_output = dlss_sr_outputs.color(frame_label);
        let (gbuffer_a_image, gbuffer_a_view) = gbuffer.a_handle(frame_label);
        let (gbuffer_b_image, gbuffer_b_view) = gbuffer.b_handle(frame_label);
        let (gbuffer_c_image, gbuffer_c_view) = gbuffer.c_handle(frame_label);
        let rr_active = dlss_rr_active(dlss_sr_mode, dlss_rr_enabled);
        // SR/RR 开启后这些输入已经在 compute graph 末尾停留在 DLSS read layout；
        // present graph 的 debug preview 必须用同一状态 import，不能再假设所有 storage image 都是 GENERAL。
        let sl_input_state = if dlss_sr_enabled(dlss_sr_mode) { DLSS_SR_INPUT_READ } else { RgImageState::GENERAL };
        let rr_input_state = if rr_active { DLSS_SR_INPUT_READ } else { RgImageState::GENERAL };
        let gbuffer_a_state = if rr_active { DLSS_SR_INPUT_READ } else { RgImageState::GENERAL };

        vec![
            debug_entry_with_state("single-frame-rt", "Single Frame RT", single_frame, sl_input_state),
            debug_entry("main-view-color", "Main View Color", main_view_color),
            debug_entry("dlss-sr-output", "DLSS SR Output", dlss_output),
            debug_entry_with_state("dlss-depth", "DLSS Depth", depth, sl_input_state),
            debug_entry_with_state("dlss-motion-vectors", "DLSS Motion Vectors", motion_vectors, sl_input_state),
            debug_entry_with_state(
                "dlss-rr-diffuse-albedo",
                "DLSS RR Diffuse Albedo",
                rr_diffuse_albedo,
                rr_input_state,
            ),
            debug_entry_with_state(
                "dlss-rr-specular-albedo",
                "DLSS RR Specular Albedo",
                rr_specular_albedo,
                rr_input_state,
            ),
            debug_entry_with_state(
                "dlss-rr-specular-motion-vectors",
                "DLSS RR Specular Motion Vectors",
                rr_specular_motion_vectors,
                rr_input_state,
            ),
            DebugImageEntry::raw_with_graph_state(
                "gbuffer-a",
                "GBuffer-A",
                gbuffer_a_image,
                gbuffer_a_view,
                GBuffer::A_FORMAT,
                gbuffer.extent(),
                gbuffer_a_state,
            ),
            DebugImageEntry::raw(
                "gbuffer-b",
                "GBuffer-B",
                gbuffer_b_image,
                gbuffer_b_view,
                GBuffer::B_FORMAT,
                gbuffer.extent(),
            ),
            DebugImageEntry::raw(
                "gbuffer-c",
                "GBuffer-C",
                gbuffer_c_image,
                gbuffer_c_view,
                GBuffer::C_FORMAT,
                gbuffer.extent(),
            ),
        ]
    }

    pub fn contribute_present_passes<'a>(
        &'a self,
        rg_builder: &mut RenderGraphBuilder<'a>,
        ctx: &'a PluginRenderCtx<'a>,
    ) -> RtPresentGraphTargets {
        let inner = self.inner();
        let record_ctx = ctx.record_ctx;
        let frame_label = record_ctx.frame_timing.frame_label();
        let main_view_targets = &inner.main_view_targets;

        // present graph 只读取 compute graph 导出的主视图 color，再 resolve 到当前 swapchain image。
        // 这里重新 import 同一个 app-owned image，让两个 graph 之间的边界保持显式。
        let color_target = main_view_targets.color(frame_label);
        let render_target = rg_builder.import_image(
            "render-target",
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

        RtPresentGraphTargets {
            present_image,
            main_view_color: render_target,
        }
    }

    fn inner(&self) -> &RtPipelineInner {
        self.inner.as_ref().expect("RtPipeline not initialized")
    }
}

fn debug_entry(id: &'static str, label: &'static str, target: ImageTarget) -> DebugImageEntry {
    DebugImageEntry::raw(id, label, target.image, target.view, target.format, target.extent)
}

fn debug_entry_with_state(
    id: &'static str,
    label: &'static str,
    target: ImageTarget,
    graph_state: RgImageState,
) -> DebugImageEntry {
    DebugImageEntry::raw_with_graph_state(
        id,
        label,
        target.image,
        target.view,
        target.format,
        target.extent,
        graph_state,
    )
}

fn dlss_sr_enabled(mode: DlssSrMode) -> bool {
    mode != DlssSrMode::Off
}

fn dlss_rr_active(mode: DlssSrMode, rr_enabled: bool) -> bool {
    mode != DlssSrMode::Off && rr_enabled
}

impl Drop for RtPipeline {
    fn drop(&mut self) {
        log::info!("RtPipeline drop");
    }
}
