use crate::gui_plugin::{DebugImageEntry, DebugImageGraphEntry};
use crate::render_pipeline::common_settings::PathTracingCommonSettings;
use crate::render_pipeline::targets::{
    DlssOutputTargets, DlssRrInputTargets, DlssSrExposureTarget, DlssSrInputTargets, ImageTarget, MainViewTargets,
    RestirDiTargets, RestirReservoirTarget, RestirSurfaceKeyTarget, RtWorkingTargets,
};
use app_render_passes::dlss_rr_pass::{DlssRrPass, DlssRrRgPass};
use app_render_passes::dlss_sr_pass::{DLSS_SR_INPUT_READ, DlssSrPass, DlssSrRgPass};
use app_render_passes::gbuffer::GBuffer;
use app_render_passes::realtime_rt_pass::{
    RealtimeRtPass, RealtimeRtRgPass, RestirReservoirRgImages, RestirSurfaceKeyRgImages,
};
use app_render_passes::resolve_pass::{ResolvePass, ResolveRgPass};
use app_render_passes::sdr_pass::{SdrPass, SdrRgPass};
use std::{cell::Cell, env};
use truvis_app_frame::plugin_api::{Plugin, PluginInitCtx, PluginRenderCtx, PluginResizeCtx, PluginShutdownCtx};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_foundation::frame_counter::{FrameCounter, FrameLabel};
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle, RgImageState};
use truvis_render_runtime::state::dlss_options::DlssOptions;

pub use crate::render_pipeline::common_settings::RtSkySamplingMode;

#[derive(Default)]
pub struct RtPipeline {
    inner: Option<RtPipelineInner>,
    /// RT app 自有的可调参数。
    ///
    /// 生命周期跟随 `RtPipeline`，由 Truvis / Cornell 等 RT app 在 ImGui update 阶段修改，
    /// 再在构建 render graph 时显式传给相关 pass。
    settings: RtPipelineSettings,
    /// ReSTIR DI 的最小 CPU history signature；用于 mode/reset 变化时切断上一帧 history。
    restir_last_mode: Cell<RtRestirDiMode>,
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
    /// Primary visible surface ReSTIR DI 模式。
    ///
    /// 这是 RT pipeline 私有 temporal lighting 开关；默认 Off，确保现有 unified NEE
    /// 路径可直接回退。reservoir history 不进入 DLSS state，也不读取 DLSS output。
    pub restir_di_mode: RtRestirDiMode,
}

impl Default for RtPipelineSettings {
    fn default() -> Self {
        Self {
            debug_channel: RtDebugChannel::Final,
            restir_di_mode: RtRestirDiMode::initial_mode_from_env(),
        }
    }
}

/// Primary visible surface ReSTIR DI 模式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RtRestirDiMode {
    /// 完全保留当前 unified NEE 路径。
    #[default]
    Off,
    /// 只生成 initial reservoir，并在 final shade 阶段重新做 visibility。
    InitialOnly,
    /// 在 initial 基础上加入上一帧 reservoir temporal reuse。
    Temporal,
    /// temporal 后追加邻域 reservoir spatial reuse。
    TemporalSpatial,
}

impl RtRestirDiMode {
    pub const ALL: [Self; 4] = [Self::Off, Self::InitialOnly, Self::Temporal, Self::TemporalSpatial];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::InitialOnly => "InitialOnly",
            Self::Temporal => "Temporal",
            Self::TemporalSpatial => "TemporalSpatial",
        }
    }

    pub fn shader_mode(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::InitialOnly => 1,
            Self::Temporal => 2,
            Self::TemporalSpatial => 3,
        }
    }

    pub fn is_enabled(self) -> bool {
        self != Self::Off
    }

    fn from_config_value(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase().replace(['_', '-', ' '], "");
        match normalized.as_str() {
            "off" => Some(Self::Off),
            "initialonly" | "initial" => Some(Self::InitialOnly),
            "temporal" => Some(Self::Temporal),
            "temporalspatial" | "spatial" => Some(Self::TemporalSpatial),
            _ => None,
        }
    }

    fn initial_mode_from_env() -> Self {
        const ENV_NAME: &str = "TRUVIS_RESTIR_DI_MODE";
        let Ok(value) = env::var(ENV_NAME) else {
            return Self::Off;
        };

        match Self::from_config_value(&value) {
            Some(mode) => {
                log::info!("Initial ReSTIR DI mode from {ENV_NAME}={value}: {mode:?}");
                mode
            }
            None => {
                log::warn!("Ignoring unsupported {ENV_NAME} value: {value}");
                Self::Off
            }
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
    /// 显示 RT shading 当前实际使用的 world-space forward normal。
    ///
    /// 该法线经过 `faceforward` 翻面，会随入射 ray 保持同侧；这是旧 `normal` 通道的兼容语义。
    ForwardNormal,
    /// 显示未经过 `faceforward` 翻面的 world-space 几何法线。
    WorldNormal,
    /// 显示 mesh object/local space 中的插值顶点法线。
    ObjectNormal,
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
    /// 显示 next-event estimation 中来自自发光三角形的直接光。
    NeeEmissive,
    /// 显示 next-event estimation 中来自 analytic light 的直接光。
    NeeAnalytic,
    /// 显示 ReSTIR DI initial reservoir 的权重强度。
    RestirInitialWeight,
    /// 显示 ReSTIR DI temporal reservoir 是否有效及 history age。
    RestirTemporalValid,
    /// 显示 ReSTIR DI final shade contribution。
    RestirFinalContribution,
}

impl RtDebugChannel {
    pub const ALL: [Self; 15] = [
        Self::Final,
        Self::ForwardNormal,
        Self::WorldNormal,
        Self::ObjectNormal,
        Self::BaseColor,
        Self::NeeHdri,
        Self::Emission,
        Self::BrdfHdri,
        Self::NeeBounce0,
        Self::NeeBounce1,
        Self::NeeEmissive,
        Self::NeeAnalytic,
        Self::RestirInitialWeight,
        Self::RestirTemporalValid,
        Self::RestirFinalContribution,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Final => "final",
            Self::ForwardNormal => "forward normal",
            Self::WorldNormal => "world normal",
            Self::ObjectNormal => "object normal",
            Self::BaseColor => "base color",
            Self::NeeHdri => "from NEE HDRI",
            Self::Emission => "from emission",
            Self::BrdfHdri => "from BRDF HDRI",
            Self::NeeBounce0 => "NEE bounce 0",
            Self::NeeBounce1 => "NEE bounce 1",
            Self::NeeEmissive => "from NEE emissive",
            Self::NeeAnalytic => "from NEE analytic",
            Self::RestirInitialWeight => "ReSTIR initial weight",
            Self::RestirTemporalValid => "ReSTIR temporal valid",
            Self::RestirFinalContribution => "ReSTIR final contribution",
        }
    }

    pub fn shader_channel(self) -> u32 {
        match self {
            Self::Final => 0,
            Self::ForwardNormal => 1,
            Self::WorldNormal => 10,
            Self::ObjectNormal => 11,
            Self::BaseColor => 2,
            Self::NeeHdri => 4,
            Self::Emission => 5,
            Self::BrdfHdri => 6,
            Self::NeeBounce0 => 7,
            Self::NeeBounce1 => 8,
            Self::NeeEmissive => 9,
            Self::NeeAnalytic => 12,
            Self::RestirInitialWeight => 13,
            Self::RestirTemporalValid => 14,
            Self::RestirFinalContribution => 15,
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
    /// Primary ReSTIR DI reservoir 与 surface-key history。
    restir_di_targets: RestirDiTargets,
    /// DLSS SR 需要的低分辨率 depth/motion-vector 输入。
    ///
    /// 即使 SR 关闭也会由 raygen 写入，便于 ImGui debug viewer 验证深度和 motion vector。
    dlss_sr_inputs: DlssSrInputTargets,
    /// DLSS SR 固定手动曝光 scale=1.0；缺少 exposure tag 时 Streamline 会退回 AutoExposure。
    dlss_sr_exposure: DlssSrExposureTarget,
    /// DLSS RR 额外需要的低分辨率输入。
    dlss_rr_inputs: DlssRrInputTargets,
    /// DLSS SR / DLAA / RR 共享输出的高分辨率 HDR color。
    dlss_outputs: DlssOutputTargets,
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
        let restir_di_targets = RestirDiTargets::new(
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
        let dlss_sr_exposure = DlssSrExposureTarget::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut *ctx.gfx_resource_manager,
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
        let dlss_outputs = DlssOutputTargets::new(
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
            restir_di_targets,
            dlss_sr_inputs,
            dlss_sr_exposure,
            dlss_rr_inputs,
            dlss_outputs,
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
        self.restir_di_targets.destroy(
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
        self.dlss_sr_exposure.destroy(
            ctx.resource_ctx,
            ctx.device_ctx,
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
        self.dlss_outputs.destroy(
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
            inner.restir_di_targets.rebuild(
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
            inner.dlss_outputs.rebuild(
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
        common_settings: &PathTracingCommonSettings,
    ) {
        let inner = self.inner();
        let record_ctx = ctx.record_ctx;
        let frame_label = record_ctx.frame_timing.frame_label();
        let rt_targets = &inner.rt_targets;
        let restir_di_targets = &inner.restir_di_targets;
        let dlss_sr_inputs = &inner.dlss_sr_inputs;
        let dlss_rr_inputs = &inner.dlss_rr_inputs;
        let dlss_outputs = &inner.dlss_outputs;
        let main_view_targets = &inner.main_view_targets;
        let debug_channel = self.settings.debug_channel.shader_channel();
        let sky_sampling_mode = common_settings.sky_sampling_mode.shader_mode();
        let sky_brightness = common_settings.sky_brightness;
        let emissive_nee_enabled = common_settings.emissive_nee_enabled;
        let analytic_nee_enabled = common_settings.analytic_nee_enabled;
        let restir_di_mode = self.settings.restir_di_mode;
        let frame_id = record_ctx.frame_timing.frame_id();
        let previous_frame_label =
            FrameLabel::from_usize((frame_id as usize + FrameCounter::fif_count() - 1) % FrameCounter::fif_count());
        // CPU 侧只负责切断明显不连续的 history：首帧、mode 变化和 DLSS reset。
        // sky/emissive/analytic light 的版本拒绝在 shader reservoir metadata 中完成，
        // 这样 resize/reset 语义留在 pipeline owner，scene 语义变化留在 GPU scene ABI。
        let restir_history_valid = restir_di_mode.is_enabled()
            && frame_id > 0
            && self.restir_last_mode.get() == restir_di_mode
            && !record_ctx.dlss_sr_state.constants().reset;
        self.restir_last_mode.set(restir_di_mode);
        let tone_mapping = common_settings.tone_mapping;

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

        // ReSTIR DI targets 是同一个 RT pass 内多次 TraceRays phase 的私有工作集。
        // initial/temporal/final 都绑定当前 frame label；history 只绑定 previous temporal，
        // 防止 spatial reuse 的邻域结果跨帧回灌到 temporal reuse。
        let restir_initial = import_restir_reservoir(
            rg_builder,
            "restir-di-initial",
            restir_di_targets.initial(frame_label),
            RgImageState::UNDEFINED_TOP,
        );
        let restir_temporal = import_restir_reservoir(
            rg_builder,
            "restir-di-temporal",
            restir_di_targets.temporal(frame_label),
            RgImageState::UNDEFINED_TOP,
        );
        let restir_final = import_restir_reservoir(
            rg_builder,
            "restir-di-final",
            restir_di_targets.final_reservoir(frame_label),
            RgImageState::UNDEFINED_TOP,
        );
        let restir_history = import_restir_reservoir(
            rg_builder,
            "restir-di-history",
            // Temporal history 必须来自上一帧 temporal reservoir，而不是 spatial/final reservoir。
            // spatial reuse 只服务当前帧出图；若把 spatial final 再喂回 temporal，会把邻域样本跨帧反馈，
            // 让 reservoir M 与相关性一起膨胀，最终在 RR 输入中形成低频彩色块。
            restir_di_targets.temporal(previous_frame_label),
            RgImageState::GENERAL,
        );
        let restir_surface = import_restir_surface_key(
            rg_builder,
            "restir-di-surface",
            restir_di_targets.surface_key(frame_label),
            RgImageState::UNDEFINED_TOP,
        );
        let restir_history_surface = import_restir_surface_key(
            rg_builder,
            "restir-di-history-surface",
            restir_di_targets.surface_key(previous_frame_label),
            RgImageState::GENERAL,
        );

        let dlss_output_target = dlss_outputs.color(frame_label);
        let dlss_output = rg_builder.import_image(
            "dlss-output",
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
                sky_sampling_mode,
                sky_brightness,
                emissive_nee_enabled,
                analytic_nee_enabled,
                restir_di_mode: restir_di_mode.shader_mode(),
                restir_history_valid,
                restir_initial,
                restir_temporal,
                restir_final,
                restir_history,
                restir_surface,
                restir_history_surface,
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

        let dlss_options = *record_ctx.dlss_options;
        if dlss_options.is_rr_active() {
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
        } else if dlss_options.is_sr_active() {
            // SR/DLAA 分支用 Streamline output 进入 SDR；不再运行传统 denoise/accum，
            // 也不在 SR 后追加第二个 upscale pass。
            let dlss_sr_exposure_target = inner.dlss_sr_exposure.exposure();
            let dlss_sr_exposure = rg_builder.import_image(
                "dlss-sr-exposure",
                dlss_sr_exposure_target.image,
                Some(dlss_sr_exposure_target.view),
                dlss_sr_exposure_target.format,
                DLSS_SR_INPUT_READ,
                None,
            );

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
                        exposure: dlss_sr_exposure,
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

    pub fn collect_debug_images(&self, frame_label: FrameLabel, dlss_options: DlssOptions) -> Vec<DebugImageEntry> {
        let inner = self.inner();
        let rt_targets = &inner.rt_targets;
        let main_view_targets = &inner.main_view_targets;
        let dlss_sr_inputs = &inner.dlss_sr_inputs;
        let dlss_rr_inputs = &inner.dlss_rr_inputs;
        let dlss_outputs = &inner.dlss_outputs;
        let gbuffer = &inner.gbuffer;

        let single_frame = rt_targets.single_frame_rt(frame_label);
        let main_view_color = main_view_targets.color(frame_label);
        let depth = dlss_sr_inputs.depth(frame_label);
        let motion_vectors = dlss_sr_inputs.motion_vectors(frame_label);
        let rr_diffuse_albedo = dlss_rr_inputs.diffuse_albedo(frame_label);
        let rr_specular_albedo = dlss_rr_inputs.specular_albedo(frame_label);
        let rr_specular_motion_vectors = dlss_rr_inputs.specular_motion_vectors(frame_label);
        let dlss_output = dlss_outputs.color(frame_label);
        let (gbuffer_a_image, gbuffer_a_view) = gbuffer.a_handle(frame_label);
        let (gbuffer_b_image, gbuffer_b_view) = gbuffer.b_handle(frame_label);
        let (gbuffer_c_image, gbuffer_c_view) = gbuffer.c_handle(frame_label);
        // SR/RR 开启后这些输入已经在 compute graph 末尾停留在 DLSS read layout；
        // present graph 的 debug preview 必须用同一状态 import，不能再假设所有 storage image 都是 GENERAL。
        let sl_input_state = if dlss_options.is_dlss_active() { DLSS_SR_INPUT_READ } else { RgImageState::GENERAL };
        let rr_input_state = if dlss_options.is_rr_active() { DLSS_SR_INPUT_READ } else { RgImageState::GENERAL };
        let gbuffer_a_state = if dlss_options.is_rr_active() { DLSS_SR_INPUT_READ } else { RgImageState::GENERAL };

        vec![
            debug_entry_with_state("single-frame-rt", "Single Frame RT", single_frame, sl_input_state),
            debug_entry("main-view-color", "Main View Color", main_view_color),
            debug_entry("dlss-output", "DLSS Output", dlss_output),
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
        _common_settings: &PathTracingCommonSettings,
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

fn import_restir_reservoir<'a>(
    rg_builder: &mut RenderGraphBuilder<'a>,
    name_prefix: &'static str,
    target: RestirReservoirTarget,
    initial_state: RgImageState,
) -> RestirReservoirRgImages {
    // 四张 image 的顺序必须和 Slang descriptor ABI 的 A/B/C/D 打包一致：
    // A/D 是 uint metadata，B/C 是 float sample/weight。这里集中导入，避免调用点手写顺序出错。
    RestirReservoirRgImages {
        a: rg_builder.import_image(
            format!("{name_prefix}-a"),
            target.a.image,
            Some(target.a.view),
            target.a.format,
            initial_state,
            None,
        ),
        b: rg_builder.import_image(
            format!("{name_prefix}-b"),
            target.b.image,
            Some(target.b.view),
            target.b.format,
            initial_state,
            None,
        ),
        c: rg_builder.import_image(
            format!("{name_prefix}-c"),
            target.c.image,
            Some(target.c.view),
            target.c.format,
            initial_state,
            None,
        ),
        d: rg_builder.import_image(
            format!("{name_prefix}-d"),
            target.d.image,
            Some(target.d.view),
            target.d.format,
            initial_state,
            None,
        ),
    }
}

fn import_restir_surface_key<'a>(
    rg_builder: &mut RenderGraphBuilder<'a>,
    name_prefix: &'static str,
    target: RestirSurfaceKeyTarget,
    initial_state: RgImageState,
) -> RestirSurfaceKeyRgImages {
    // surface key 的 A/B/C 三张 RGBA32F 图像是 ReSTIR 的高精度 primary surface history。
    // 它和 RR/SR GBuffer 不是同一契约，不能在 helper 中合并或改用压缩 GBuffer 资源。
    RestirSurfaceKeyRgImages {
        a: rg_builder.import_image(
            format!("{name_prefix}-a"),
            target.a.image,
            Some(target.a.view),
            target.a.format,
            initial_state,
            None,
        ),
        b: rg_builder.import_image(
            format!("{name_prefix}-b"),
            target.b.image,
            Some(target.b.view),
            target.b.format,
            initial_state,
            None,
        ),
        c: rg_builder.import_image(
            format!("{name_prefix}-c"),
            target.c.image,
            Some(target.c.view),
            target.c.format,
            initial_state,
            None,
        ),
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

impl Drop for RtPipeline {
    fn drop(&mut self) {
        log::info!("RtPipeline drop");
    }
}
