use ash::vk;

use truvis_frame_api::plugin::{Plugin, PluginInitCtx, PluginRenderCtx, PluginShutdownCtx};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxDeviceInfoCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::swapchain::swapchain::GfxSwapchainImageInfo;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle, RgImageState, RgSemaphoreInfo};
use truvis_render_interface::cmd_allocator::CmdAllocator;
use truvis_render_interface::fif_buffer::FifBuffers;
use truvis_render_interface::frame_counter::FrameCounter;
use truvis_render_interface::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_interface::pipeline_settings::FrameLabel;
use truvis_render_passes::blit_pass::{BlitPass, BlitRgPass};
use truvis_render_passes::denoise_accum_pass::{DenoiseAccumPass, DenoiseAccumRgPass};
use truvis_render_passes::realtime_rt_pass::{RealtimeRtPass, RealtimeRtRgPass};
use truvis_render_passes::resolve_pass::{ResolvePass, ResolveRgPass};
use truvis_render_passes::sdr_pass::{SdrPass, SdrRgPass};

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
    compute_cmds: [GfxCommandBuffer; FrameCounter::fif_count()],
    present_cmds: [GfxCommandBuffer; FrameCounter::fif_count()],
}

impl RtPipelineInner {
    fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        device_info_ctx: GfxDeviceInfoCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        global_descriptor_sets: &GlobalDescriptorSets,
        swapchain_image_info: GfxSwapchainImageInfo,
        cmd_allocator: &mut CmdAllocator,
    ) -> Self {
        let realtime_rt_pass =
            RealtimeRtPass::new(resource_ctx, device_ctx, device_info_ctx, immediate_ctx, global_descriptor_sets);
        let denoise_accum_pass = DenoiseAccumPass::new(device_ctx, global_descriptor_sets);
        let blit_pass = BlitPass::new(device_ctx, global_descriptor_sets);
        let sdr_pass = SdrPass::new(device_ctx, global_descriptor_sets);
        let resolve_pass = ResolvePass::new(device_ctx, global_descriptor_sets, swapchain_image_info.image_format);

        let compute_cmds = FrameCounter::frame_labes()
            .map(|frame_label| cmd_allocator.alloc_command_buffer(device_ctx, frame_label, "rt-compute-subgraph"));
        let present_cmds = FrameCounter::frame_labes()
            .map(|frame_label| cmd_allocator.alloc_command_buffer(device_ctx, frame_label, "rt-present-subgraph"));

        Self {
            realtime_rt_pass,
            denoise_accum_pass,
            blit_pass,
            sdr_pass,
            resolve_pass,
            compute_cmds,
            present_cmds,
        }
    }

    fn destroy(self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        self.realtime_rt_pass.destroy(resource_ctx, device_ctx);
        self.denoise_accum_pass.destroy(device_ctx);
        self.blit_pass.destroy(device_ctx);
        self.sdr_pass.destroy(device_ctx);
        self.resolve_pass.destroy(device_ctx);
    }
}

impl Plugin for RtPipeline {
    fn init(&mut self, ctx: &mut PluginInitCtx) {
        self.inner = Some(RtPipelineInner::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.device_info_ctx,
            ctx.immediate_ctx,
            &ctx.render_world.global_descriptor_sets,
            ctx.render_present.swapchain_image_info(),
            ctx.cmd_allocator,
        ));
    }

    fn shutdown(&mut self, ctx: &mut PluginShutdownCtx<'_>) {
        if let Some(inner) = self.inner.take() {
            inner.destroy(ctx.resource_ctx, ctx.device_ctx);
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
        let render_world = ctx.render_world;
        let frame_label = render_world.frame_counter.frame_label();
        let fif_buffers = &render_world.fif_buffers;

        let (single_frame_image_handle, single_frame_view_handle) = fif_buffers.single_frame_rt_handle(frame_label);
        let single_frame_image = rg_builder.import_image(
            "single-frame-image",
            single_frame_image_handle,
            Some(single_frame_view_handle),
            fif_buffers.single_frame_rt_format(),
            RgImageState::UNDEFINED_TOP,
            None,
        );

        let (gbuffer_a_image_handle, gbuffer_a_view_handle) = fif_buffers.gbuffer_a_handle(frame_label);
        let gbuffer_a = rg_builder.import_image(
            "gbuffer-a",
            gbuffer_a_image_handle,
            Some(gbuffer_a_view_handle),
            FifBuffers::gbuffer_a_format(),
            RgImageState::UNDEFINED_TOP,
            None,
        );

        let (gbuffer_b_image_handle, gbuffer_b_view_handle) = fif_buffers.gbuffer_b_handle(frame_label);
        let gbuffer_b = rg_builder.import_image(
            "gbuffer-b",
            gbuffer_b_image_handle,
            Some(gbuffer_b_view_handle),
            FifBuffers::gbuffer_b_format(),
            RgImageState::UNDEFINED_TOP,
            None,
        );

        let (gbuffer_c_image_handle, gbuffer_c_view_handle) = fif_buffers.gbuffer_c_handle(frame_label);
        let gbuffer_c = rg_builder.import_image(
            "gbuffer-c",
            gbuffer_c_image_handle,
            Some(gbuffer_c_view_handle),
            FifBuffers::gbuffer_c_format(),
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
                    render_world,
                    render_scene: ctx.render_scene,
                    single_frame_image,
                    single_frame_extent: render_world.frame_settings.frame_extent,
                    gbuffer_a,
                    gbuffer_b,
                    gbuffer_c,
                },
            )
            .add_pass(
                "denoise-accum",
                DenoiseAccumRgPass {
                    denoise_accum_pass: &inner.denoise_accum_pass,
                    render_world,
                    single_frame_image,
                    accum_image,
                    gbuffer_a,
                    gbuffer_b,
                    gbuffer_c,
                    image_extent: render_world.frame_settings.frame_extent,
                },
            )
            .add_pass(
                "blit",
                BlitRgPass {
                    blit_pass: &inner.blit_pass,
                    render_world,
                    src_image: accum_image,
                    dst_image: render_target,
                    src_image_extent: render_world.frame_settings.frame_extent,
                    dst_image_extent: render_world.frame_settings.frame_extent,
                },
            )
            .add_pass(
                "hdr-to-sdr",
                SdrRgPass {
                    sdr_pass: &inner.sdr_pass,
                    render_world,
                    src_image: accum_image,
                    dst_image: render_target,
                    src_image_extent: render_world.frame_settings.frame_extent,
                    dst_image_extent: render_world.frame_settings.frame_extent,
                },
            );
    }

    pub fn contribute_present_passes<'a>(
        &'a self,
        rg_builder: &mut RenderGraphBuilder<'a>,
        ctx: &'a PluginRenderCtx<'a>,
    ) -> RgImageHandle {
        let inner = self.inner();
        let render_world = ctx.render_world;
        let render_present = ctx.render_present;
        let frame_label = render_world.frame_counter.frame_label();
        let fif_buffers = &render_world.fif_buffers;

        let (render_target_image_handle, render_target_view_handle) = fif_buffers.render_target_handle(frame_label);
        let render_target = rg_builder.import_image(
            "render-target",
            render_target_image_handle,
            Some(render_target_view_handle),
            fif_buffers.render_target_format(),
            RgImageState::SHADER_READ_FRAGMENT,
            None,
        );

        let present_target = render_present.current_target(frame_label);
        let present_image = rg_builder.import_image(
            "present-image",
            present_target.render_target_image_handle,
            Some(present_target.render_target_view_handle),
            present_target.swapchain_image_info.image_format,
            RgImageState::UNDEFINED_BOTTOM,
            Some(RgSemaphoreInfo::binary(
                present_target.present_complete_semaphore.handle(),
                vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
            )),
        );

        rg_builder.export_image(
            present_image,
            RgImageState::PRESENT_BOTTOM,
            Some(RgSemaphoreInfo::binary(
                present_target.render_complete_semaphore.handle(),
                vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
            )),
        );

        rg_builder.add_pass(
            "resolve",
            ResolveRgPass {
                resolve_pass: &inner.resolve_pass,
                render_world,
                render_target,
                swapchain_image: present_image,
                swapchain_extent: present_target.swapchain_image_info.image_extent,
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
