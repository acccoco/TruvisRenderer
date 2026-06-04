use crate::gui_plugin::{DebugImageEntry, DebugImageGraphEntry};
use crate::render_pipeline::targets::{
    DlssSrInputTargets, DlssSrOutputTargets, ImageTarget, MainViewTargets, RtWorkingTargets,
};
use app_render_passes::dlss_sr_pass::{DLSS_SR_INPUT_READ, DlssSrPass, DlssSrRgPass};
use app_render_passes::gbuffer::GBuffer;
use app_render_passes::realtime_rt_pass::{RealtimeRtPass, RealtimeRtRgPass};
use app_render_passes::resolve_pass::{ResolvePass, ResolveRgPass};
use app_render_passes::sdr_pass::{SdrPass, SdrRgPass};
use truvis_app_frame::plugin_api::{Plugin, PluginInitCtx, PluginRenderCtx, PluginResizeCtx, PluginShutdownCtx};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxResourceCtx};
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_foundation::frame_counter::FrameCounter;
use truvis_render_foundation::gpu_store::GpuStore;
use truvis_render_foundation::pipeline_settings::{DlssSrMode, FrameLabel};
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle, RgImageState};

#[derive(Default)]
pub struct RtPipeline {
    inner: Option<RtPipelineInner>,
}

struct RtPipelineInner {
    realtime_rt_pass: RealtimeRtPass,
    /// DLSS SR 是外部 opaque pass，不拥有 shader pipeline；只在 SR/DLAA 分支被加入 compute graph。
    dlss_sr_pass: DlssSrPass,
    sdr_pass: SdrPass,
    resolve_pass: ResolvePass,
    gbuffer: GBuffer,
    /// RT 私有工作图像。它们的格式/用途由 RT pipeline 决定，因此不再放在 engine `GpuStore`。
    rt_targets: RtWorkingTargets,
    /// DLSS SR 需要的低分辨率 depth/motion-vector 输入。
    ///
    /// 即使 SR 关闭也会由 raygen 写入，便于 ImGui debug viewer 验证深度和 motion vector。
    dlss_sr_inputs: DlssSrInputTargets,
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
            &ctx.gpu_store.global_descriptor_sets,
        );
        let dlss_sr_pass = DlssSrPass::new();
        let sdr_pass = SdrPass::new(ctx.device_ctx, &ctx.gpu_store.global_descriptor_sets);
        let resolve_pass = ResolvePass::new(
            ctx.device_ctx,
            &ctx.gpu_store.global_descriptor_sets,
            ctx.present.swapchain_image_info().image_format,
        );
        // `RenderRuntime::new` 早于窗口创建，只能给 `FrameSettings` 一个占位 extent；
        // app-owned target 必须使用 init 阶段已经创建好的 swapchain extent，避免首帧按 400x400
        // 创建中间图像。runtime 会在 `init_after_window` 同步该值，这里仍显式覆盖，保证契约局部可见。
        let mut target_frame_settings = ctx.gpu_store.frame_settings;
        target_frame_settings.set_native_extent(ctx.swapchain_image_info.image_extent);

        let rt_targets = RtWorkingTargets::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut ctx.gpu_store.gfx_resource_manager,
            &mut ctx.gpu_store.bindless_manager,
            &target_frame_settings,
            &ctx.gpu_store.frame_counter,
        );
        let main_view_targets = MainViewTargets::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut ctx.gpu_store.gfx_resource_manager,
            &mut ctx.gpu_store.bindless_manager,
            &target_frame_settings,
            &ctx.gpu_store.frame_counter,
        );
        let dlss_sr_inputs = DlssSrInputTargets::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut ctx.gpu_store.gfx_resource_manager,
            &mut ctx.gpu_store.bindless_manager,
            &target_frame_settings,
            &ctx.gpu_store.frame_counter,
        );
        let dlss_sr_outputs = DlssSrOutputTargets::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut ctx.gpu_store.gfx_resource_manager,
            &mut ctx.gpu_store.bindless_manager,
            &target_frame_settings,
            &ctx.gpu_store.frame_counter,
        );

        let gbuffer = GBuffer::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut ctx.gpu_store.gfx_resource_manager,
            &mut ctx.gpu_store.bindless_manager,
            target_frame_settings.render_extent,
            &ctx.gpu_store.frame_counter,
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
            sdr_pass,
            resolve_pass,
            gbuffer,
            rt_targets,
            dlss_sr_inputs,
            dlss_sr_outputs,
            main_view_targets,
            compute_cmds,
            present_cmds,
        }
    }

    fn destroy(mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>, gpu_store: &mut GpuStore) {
        // pass pipeline 本身只依赖 device；target image/view 依赖 resource manager 和 bindless。
        // shutdown 阶段 runtime 已经 wait idle，先销毁 pipeline 再释放 target 不会影响 GPU 引用安全，
        // 但 target 仍必须在 runtime `GfxResourceManager` 销毁前显式释放。
        self.realtime_rt_pass.destroy(resource_ctx, device_ctx);
        self.dlss_sr_pass.destroy();
        self.sdr_pass.destroy(device_ctx);
        self.resolve_pass.destroy(device_ctx);
        self.gbuffer.destroy(
            resource_ctx,
            device_ctx,
            &mut gpu_store.bindless_manager,
            &mut gpu_store.gfx_resource_manager,
            DestroyReason::Shutdown,
        );
        self.rt_targets.destroy(
            resource_ctx,
            device_ctx,
            &mut gpu_store.bindless_manager,
            &mut gpu_store.gfx_resource_manager,
            DestroyReason::Shutdown,
        );
        self.dlss_sr_inputs.destroy(
            resource_ctx,
            device_ctx,
            &mut gpu_store.bindless_manager,
            &mut gpu_store.gfx_resource_manager,
            DestroyReason::Shutdown,
        );
        self.dlss_sr_outputs.destroy(
            resource_ctx,
            device_ctx,
            &mut gpu_store.bindless_manager,
            &mut gpu_store.gfx_resource_manager,
            DestroyReason::Shutdown,
        );
        self.main_view_targets.destroy(
            resource_ctx,
            device_ctx,
            &mut gpu_store.bindless_manager,
            &mut gpu_store.gfx_resource_manager,
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
            let target_frame_settings = ctx.gpu_store.frame_settings;
            inner.rt_targets.rebuild(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.immediate_ctx,
                &mut ctx.gpu_store.bindless_manager,
                &mut ctx.gpu_store.gfx_resource_manager,
                &target_frame_settings,
                &ctx.gpu_store.frame_counter,
            );
            inner.dlss_sr_inputs.rebuild(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.immediate_ctx,
                &mut ctx.gpu_store.bindless_manager,
                &mut ctx.gpu_store.gfx_resource_manager,
                &target_frame_settings,
                &ctx.gpu_store.frame_counter,
            );
            inner.dlss_sr_outputs.rebuild(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.immediate_ctx,
                &mut ctx.gpu_store.bindless_manager,
                &mut ctx.gpu_store.gfx_resource_manager,
                &target_frame_settings,
                &ctx.gpu_store.frame_counter,
            );
            inner.main_view_targets.rebuild(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.immediate_ctx,
                &mut ctx.gpu_store.bindless_manager,
                &mut ctx.gpu_store.gfx_resource_manager,
                &target_frame_settings,
                &ctx.gpu_store.frame_counter,
            );
            inner.gbuffer.rebuild(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.immediate_ctx,
                &mut ctx.gpu_store.bindless_manager,
                &mut ctx.gpu_store.gfx_resource_manager,
                target_frame_settings.render_extent,
                &ctx.gpu_store.frame_counter,
            );
        }
    }

    fn shutdown(&mut self, ctx: &mut PluginShutdownCtx<'_>) {
        if let Some(inner) = self.inner.take() {
            inner.destroy(ctx.resource_ctx, ctx.device_ctx, ctx.gpu_store);
        }
    }
}

impl RtPipeline {
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
        let gpu_store = ctx.gpu_store;
        let frame_label = gpu_store.frame_counter.frame_label();
        let rt_targets = &inner.rt_targets;
        let dlss_sr_inputs = &inner.dlss_sr_inputs;
        let dlss_sr_outputs = &inner.dlss_sr_outputs;
        let main_view_targets = &inner.main_view_targets;

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
                gpu_store,
                render_scene: ctx.render_scene,
                single_frame_image,
                single_frame_extent: gpu_store.frame_settings.render_extent,
                gbuffer_a,
                gbuffer_b,
                gbuffer_c,
                depth,
                motion_vectors,
            },
        );

        if dlss_sr_enabled(gpu_store.pipeline_settings.dlss_sr_mode) {
            // SR/DLAA 分支用 Streamline output 进入 SDR；不再运行传统 denoise/accum，
            // 也不在 SR 后追加第二个 upscale pass。
            rg_builder
                .add_pass(
                    "dlss-sr",
                    DlssSrRgPass {
                        dlss_sr_pass: &inner.dlss_sr_pass,
                        gpu_store,
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
                        gpu_store,
                        src_image: dlss_output,
                        dst_image: render_target,
                        src_image_extent: gpu_store.frame_settings.output_extent,
                        dst_image_extent: gpu_store.frame_settings.output_extent,
                    },
                );
        } else {
            // Native fallback 直接把低分辨率/原生 RT color 送入 SDR。此时 render/output extent
            // 通常相等；若未来支持非 DLSS upscale，这里需要重新明确尺寸契约。
            rg_builder.add_pass(
                "hdr-to-sdr",
                SdrRgPass {
                    sdr_pass: &inner.sdr_pass,
                    gpu_store,
                    src_image: single_frame_image,
                    dst_image: render_target,
                    src_image_extent: gpu_store.frame_settings.render_extent,
                    dst_image_extent: gpu_store.frame_settings.output_extent,
                },
            );
        }
    }

    pub fn collect_debug_images(&self, frame_label: FrameLabel, dlss_sr_mode: DlssSrMode) -> Vec<DebugImageEntry> {
        let inner = self.inner();
        let rt_targets = &inner.rt_targets;
        let main_view_targets = &inner.main_view_targets;
        let dlss_sr_inputs = &inner.dlss_sr_inputs;
        let dlss_sr_outputs = &inner.dlss_sr_outputs;
        let gbuffer = &inner.gbuffer;

        let single_frame = rt_targets.single_frame_rt(frame_label);
        let main_view_color = main_view_targets.color(frame_label);
        let depth = dlss_sr_inputs.depth(frame_label);
        let motion_vectors = dlss_sr_inputs.motion_vectors(frame_label);
        let dlss_output = dlss_sr_outputs.color(frame_label);
        let (gbuffer_a_image, gbuffer_a_view) = gbuffer.a_handle(frame_label);
        let (gbuffer_b_image, gbuffer_b_view) = gbuffer.b_handle(frame_label);
        let (gbuffer_c_image, gbuffer_c_view) = gbuffer.c_handle(frame_label);
        // SR 开启后这些输入已经在 compute graph 末尾停留在 DLSS read layout；
        // present graph 的 debug preview 必须用同一状态 import，不能再假设所有 storage image 都是 GENERAL。
        let sr_input_state = if dlss_sr_enabled(dlss_sr_mode) { DLSS_SR_INPUT_READ } else { RgImageState::GENERAL };

        vec![
            debug_entry_with_state("single-frame-rt", "Single Frame RT", single_frame, sr_input_state),
            debug_entry("main-view-color", "Main View Color", main_view_color),
            debug_entry("dlss-sr-output", "DLSS SR Output", dlss_output),
            debug_entry_with_state("dlss-depth", "DLSS Depth", depth, sr_input_state),
            debug_entry_with_state("dlss-motion-vectors", "DLSS Motion Vectors", motion_vectors, sr_input_state),
            DebugImageEntry::raw(
                "gbuffer-a",
                "GBuffer-A",
                gbuffer_a_image,
                gbuffer_a_view,
                GBuffer::A_FORMAT,
                gbuffer.extent(),
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
        let gpu_store = ctx.gpu_store;
        let frame_label = gpu_store.frame_counter.frame_label();
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
                gpu_store,
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

impl Drop for RtPipeline {
    fn drop(&mut self) {
        log::info!("RtPipeline drop");
    }
}
