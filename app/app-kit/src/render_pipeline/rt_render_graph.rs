use app_render_passes::blit_pass::{BlitPass, BlitRgPass};
use app_render_passes::denoise_accum_pass::{DenoiseAccumPass, DenoiseAccumRgPass};
use app_render_passes::gbuffer::GBuffer;
use app_render_passes::realtime_rt_pass::{RealtimeRtPass, RealtimeRtRgPass};
use app_render_passes::resolve_pass::{ResolvePass, ResolveRgPass};
use app_render_passes::sdr_pass::{SdrPass, SdrRgPass};
use truvis_app_frame::plugin_api::{Plugin, PluginInitCtx, PluginRenderCtx, PluginResizeCtx, PluginShutdownCtx};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxResourceCtx};
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

        let gbuffer = GBuffer::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut ctx.gpu_store.gfx_resource_manager,
            &mut ctx.gpu_store.bindless_manager,
            ctx.swapchain_image_info.image_extent,
            &ctx.gpu_store.frame_counter,
        );

        let compute_cmds = FrameCounter::frame_labes()
            .map(|frame_label| ctx.cmd_allocator.alloc_command_buffer(ctx.device_ctx, frame_label, "rt-compute-subgraph"));
        let present_cmds = FrameCounter::frame_labes()
            .map(|frame_label| ctx.cmd_allocator.alloc_command_buffer(ctx.device_ctx, frame_label, "rt-present-subgraph"));

        Self {
            realtime_rt_pass,
            denoise_accum_pass,
            blit_pass,
            sdr_pass,
            resolve_pass,
            gbuffer,
            compute_cmds,
            present_cmds,
        }
    }

    fn destroy(mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>, gpu_store: &mut GpuStore) {
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
            truvis_gfx::resources::lifecycle::DestroyReason::Shutdown,
        );
    }
}

impl Plugin for RtPipeline {
    fn init(&mut self, ctx: &mut PluginInitCtx) {
        self.inner = Some(RtPipelineInner::new(ctx));
    }

    fn on_resize(&mut self, ctx: &mut PluginResizeCtx) {
        if let Some(inner) = self.inner.as_mut() {
            inner.gbuffer.rebuild(
                ctx.resource_ctx,
                ctx.device_ctx,
                ctx.immediate_ctx,
                &mut ctx.gpu_store.bindless_manager,
                &mut ctx.gpu_store.gfx_resource_manager,
                ctx.gpu_store.frame_settings.frame_extent,
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
        let fif_buffers = &gpu_store.fif_buffers;

        let (single_frame_image_handle, single_frame_view_handle) = fif_buffers.single_frame_rt_handle(frame_label);
        let single_frame_image = rg_builder.import_image(
            "single-frame-image",
            single_frame_image_handle,
            Some(single_frame_view_handle),
            fif_buffers.single_frame_rt_format(),
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

        let accum_image = rg_builder.import_image(
            "accum-image",
            fif_buffers.accum_image_handle(),
            Some(fif_buffers.accum_image_view_handle()),
            fif_buffers.accum_image_format(),
            RgImageState::STORAGE_READ_WRITE_COMPUTE,
            None,
        );

        let (render_target_image_handle, render_target_view_handle) = fif_buffers.render_target_handle(frame_label);
        let render_target = rg_builder.import_image(
            "render-target",
            render_target_image_handle,
            Some(render_target_view_handle),
            fif_buffers.render_target_format(),
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
        let fif_buffers = &gpu_store.fif_buffers;

        let (render_target_image_handle, render_target_view_handle) = fif_buffers.render_target_handle(frame_label);
        let render_target = rg_builder.import_image(
            "render-target",
            render_target_image_handle,
            Some(render_target_view_handle),
            fif_buffers.render_target_format(),
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
