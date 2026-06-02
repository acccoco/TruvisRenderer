use crate::render_pipeline::targets::{MainViewTargets, RtWorkingTargets};
use app_render_passes::blit_pass::{BlitPass, BlitRgPass};
use app_render_passes::denoise_accum_pass::{DenoiseAccumPass, DenoiseAccumRgPass};
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
use truvis_render_foundation::pipeline_settings::FrameLabel;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle, RgImageState};

#[derive(Default)]
pub struct RtPipeline {
    inner: Option<RtPipelineInner>,
}

struct RtPipelineInner {
    realtime_rt_pass: RealtimeRtPass,
    denoise_accum_pass: DenoiseAccumPass,
    blit_pass: BlitPass,
    sdr_pass: SdrPass,
    resolve_pass: ResolvePass,
    gbuffer: GBuffer,
    /// RT 私有工作图像。它们的格式/用途由 RT pipeline 决定，因此不再放在 engine `GpuStore`。
    rt_targets: RtWorkingTargets,
    /// 主视图离屏目标。compute graph 写入 color，present graph 再 resolve 到 swapchain。
    main_view_targets: MainViewTargets,
    compute_cmds: [GfxCommandBuffer; FrameCounter::fif_count()],
    present_cmds: [GfxCommandBuffer; FrameCounter::fif_count()],
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
        let denoise_accum_pass = DenoiseAccumPass::new(ctx.device_ctx, &ctx.gpu_store.global_descriptor_sets);
        let blit_pass = BlitPass::new(ctx.device_ctx, &ctx.gpu_store.global_descriptor_sets);
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
        target_frame_settings.frame_extent = ctx.swapchain_image_info.image_extent;

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

        let gbuffer = GBuffer::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut ctx.gpu_store.gfx_resource_manager,
            &mut ctx.gpu_store.bindless_manager,
            target_frame_settings.frame_extent,
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
            denoise_accum_pass,
            blit_pass,
            sdr_pass,
            resolve_pass,
            gbuffer,
            rt_targets,
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
        self.denoise_accum_pass.destroy(device_ctx);
        self.blit_pass.destroy(device_ctx);
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
            let mut target_frame_settings = ctx.gpu_store.frame_settings;
            target_frame_settings.frame_extent = ctx.present.swapchain_image_info().image_extent;
            inner.rt_targets.rebuild(
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
                target_frame_settings.frame_extent,
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

        let accum_target = rt_targets.accum();
        let accum_image = rg_builder.import_image(
            "accum-image",
            accum_target.image,
            Some(accum_target.view),
            accum_target.format,
            RgImageState::STORAGE_READ_WRITE_COMPUTE,
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

        rg_builder
            .add_pass(
                "ray-tracing",
                RealtimeRtRgPass {
                    rt_pass: &inner.realtime_rt_pass,
                    gpu_store,
                    render_scene: ctx.render_scene,
                    single_frame_image,
                    single_frame_extent: gpu_store.frame_settings.frame_extent,
                    gbuffer_a,
                    gbuffer_b,
                    gbuffer_c,
                },
            )
            .add_pass(
                "denoise-accum",
                DenoiseAccumRgPass {
                    denoise_accum_pass: &inner.denoise_accum_pass,
                    gpu_store,
                    single_frame_image,
                    accum_image,
                    gbuffer_a,
                    gbuffer_b,
                    gbuffer_c,
                    image_extent: gpu_store.frame_settings.frame_extent,
                },
            )
            .add_pass(
                "blit",
                BlitRgPass {
                    blit_pass: &inner.blit_pass,
                    gpu_store,
                    src_image: accum_image,
                    dst_image: render_target,
                    src_image_extent: gpu_store.frame_settings.frame_extent,
                    dst_image_extent: gpu_store.frame_settings.frame_extent,
                },
            )
            .add_pass(
                "hdr-to-sdr",
                SdrRgPass {
                    sdr_pass: &inner.sdr_pass,
                    gpu_store,
                    src_image: accum_image,
                    dst_image: render_target,
                    src_image_extent: gpu_store.frame_settings.frame_extent,
                    dst_image_extent: gpu_store.frame_settings.frame_extent,
                },
            );
    }

    pub fn contribute_present_passes<'a>(
        &'a self,
        rg_builder: &mut RenderGraphBuilder<'a>,
        ctx: &'a PluginRenderCtx<'a>,
    ) -> RgImageHandle {
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

        present_image
    }

    fn inner(&self) -> &RtPipelineInner {
        self.inner.as_ref().expect("RtPipeline not initialized")
    }
}

impl Drop for RtPipeline {
    fn drop(&mut self) {
        log::info!("RtPipeline drop");
    }
}
