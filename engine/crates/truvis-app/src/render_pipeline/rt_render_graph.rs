use ash::vk;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::gfx::Gfx;
use truvis_gfx::swapchain::swapchain::GfxSwapchain;
use truvis_gui_backend::gui_pass::GuiPass;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageState, RgSemaphoreInfo};
use truvis_render_interface::fif_buffer::FifBuffers;
use truvis_render_interface::cmd_allocator::CmdAllocator;
use truvis_render_interface::frame_counter::FrameCounter;
use truvis_render_interface::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_passes::blit_pass::{BlitPass, BlitRgPass};
use truvis_render_passes::denoise_accum_pass::{DenoiseAccumPass, DenoiseAccumRgPass};
use truvis_render_passes::realtime_rt_pass::{RealtimeRtPass, RealtimeRtRgPass};
use truvis_render_passes::resolve_pass::{ResolvePass, ResolveRgPass};
use truvis_render_passes::sdr_pass::{SdrPass, SdrRgPass};
use truvis_render_interface::render_world::RenderWorld;
use truvis_renderer::present::render_present::RenderPresent;

use crate::gui_rg_pass::GuiRgPass;

pub struct RtPipeline {
    /// 光追 pass
    realtime_rt_pass: RealtimeRtPass,
    /// 降噪累积 pass（双边滤波降噪 + 时域累积）
    denoise_accum_pass: DenoiseAccumPass,
    /// Blit pass
    blit_pass: BlitPass,
    /// SDR pass
    sdr_pass: SdrPass,
    resolve_pass: ResolvePass,
    gui_pass: GuiPass,

    compute_cmds: [GfxCommandBuffer; FrameCounter::fif_count()],
    present_cmds: [GfxCommandBuffer; FrameCounter::fif_count()],
}

// new & init
impl RtPipeline {
    /// 创建新的 RT 渲染管线
    pub fn new(
        global_descriptor_sets: &GlobalDescriptorSets,
        swapchain: &GfxSwapchain,
        cmd_allocator: &mut CmdAllocator,
    ) -> Self {
        let realtime_rt_pass = RealtimeRtPass::new(global_descriptor_sets);
        let denoise_accum_pass = DenoiseAccumPass::new(global_descriptor_sets);
        let blit_pass = BlitPass::new(global_descriptor_sets);
        let sdr_pass = SdrPass::new(global_descriptor_sets);
        let resolve_pass = ResolvePass::new(global_descriptor_sets, swapchain.image_infos().image_format);
        let gui_pass = GuiPass::new(global_descriptor_sets, swapchain.image_infos().image_format);

        let compute_cmds = FrameCounter::frame_labes()
            .map(|frame_label| cmd_allocator.alloc_command_buffer(frame_label, "rt-compute-subgraph"));
        let present_cmds = FrameCounter::frame_labes()
            .map(|frame_label| cmd_allocator.alloc_command_buffer(frame_label, "rt-present-subgraph"));

        Self {
            realtime_rt_pass,
            denoise_accum_pass,
            blit_pass,
            sdr_pass,
            resolve_pass,
            gui_pass,
            compute_cmds,
            present_cmds,
        }
    }
}

// render
impl RtPipeline {
    pub fn render(
        &self,
        render_world: &RenderWorld,
        render_present: &RenderPresent,
        gui_draw_data: &imgui::DrawData,
        frame_fence: &GfxSemaphore,
    ) {
        let frame_label = render_world.frame_counter.frame_label();
        let frame_id = render_world.frame_counter.frame_id();

        // compute subgraph
        let compute_subgraph_submit = {
            let mut compute_graph_builder = RenderGraphBuilder::new();
            self.prepare_compute_graph(&mut compute_graph_builder, render_world);
            let compute_graph = compute_graph_builder.compile();

            // 调试输出执行计划
            if log::log_enabled!(log::Level::Debug) {
                static PRINT_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
                PRINT_DEBUG_INFO.call_once(|| {
                    compute_graph.print_execution_plan();
                });
            }

            let compute_cmd = &self.compute_cmds[*frame_label];
            compute_cmd.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "rt-render-graph");
            compute_graph.execute(compute_cmd, &render_world.gfx_resource_manager);
            compute_cmd.end();

            compute_graph.build_submit_info(std::slice::from_ref(compute_cmd))
        };

        // present subgraph
        let present_subgraph_submit = {
            let mut present_graph_builder = RenderGraphBuilder::new();
            present_graph_builder.signal_semaphore(RgSemaphoreInfo::timeline(
                frame_fence.handle(),
                vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
                frame_id,
            ));

            self.prepare_present_graph(&mut present_graph_builder, render_world, render_present, gui_draw_data);
            let present_graph = present_graph_builder.compile();

            // 调试输出执行计划
            if log::log_enabled!(log::Level::Debug) {
                static PRINT_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
                PRINT_DEBUG_INFO.call_once(|| {
                    present_graph.print_execution_plan();
                });
            }

            let present_cmd = &self.present_cmds[*frame_label];
            present_cmd.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "rt-present-graph");
            present_graph.execute(present_cmd, &render_world.gfx_resource_manager);
            present_cmd.end();

            present_graph.build_submit_info(std::slice::from_ref(present_cmd))
        };

        Gfx::get().gfx_queue().submit(vec![compute_subgraph_submit, present_subgraph_submit], None);
    }

    pub fn prepare_compute_graph<'a>(
        &'a self,
        rg_builder: &mut RenderGraphBuilder<'a>,
        render_world: &'a RenderWorld,
    ) {
        let frame_label = render_world.frame_counter.frame_label();
        let fif_buffers = &render_world.fif_buffers;

        // 导入外部资源
        // 单帧 RT 输出（per-frame image）
        let (single_frame_image_handle, single_frame_view_handle) = fif_buffers.single_frame_rt_handle(frame_label);
        let single_frame_image = rg_builder.import_image(
            "single-frame-image",
            single_frame_image_handle,
            Some(single_frame_view_handle),
            fif_buffers.single_frame_rt_format(),
            RgImageState::UNDEFINED_TOP,
            None,
        );

        // ========== GBuffer 导入 ==========
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

        // 累积图像（跨帧持久）
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

        // 导出渲染目标（用于后续绘制）
        rg_builder.export_image(render_target, RgImageState::SHADER_READ_FRAGMENT, None);

        // 添加 pass
        // 流程: ray-tracing → denoise-accum → blit → hdr-to-sdr
        rg_builder
            .add_pass(
                "ray-tracing",
                RealtimeRtRgPass {
                    rt_pass: &self.realtime_rt_pass,
                    render_world,
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
                    denoise_accum_pass: &self.denoise_accum_pass,
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
                    blit_pass: &self.blit_pass,
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
                    sdr_pass: &self.sdr_pass,
                    render_world,
                    src_image: accum_image,
                    dst_image: render_target,
                    src_image_extent: render_world.frame_settings.frame_extent,
                    dst_image_extent: render_world.frame_settings.frame_extent,
                },
            );
    }

    pub fn prepare_present_graph<'a>(
        &'a self,
        rg_builder: &mut RenderGraphBuilder<'a>,
        render_world: &'a RenderWorld,
        render_present: &'a RenderPresent,
        gui_draw_data: &'a imgui::DrawData,
    ) {
        let frame_label = render_world.frame_counter.frame_label();
        let fif_buffers = &render_world.fif_buffers;

        // 导入外部资源
        let (render_target_image_handle, render_target_view_handle) = fif_buffers.render_target_handle(frame_label);
        let render_target = rg_builder.import_image(
            "render-target",
            render_target_image_handle,
            Some(render_target_view_handle),
            fif_buffers.render_target_format(),
            RgImageState::SHADER_READ_FRAGMENT,
            None,
        );

        let (present_image, present_view) = render_present.current_image_and_view();
        let present_image = rg_builder.import_image(
            "present-image",
            present_image,
            Some(present_view),
            render_present.swapchain_image_info().image_format,
            RgImageState::UNDEFINED_BOTTOM,
            Some(RgSemaphoreInfo::binary(
                render_present.current_present_complete_semaphore(frame_label).handle(),
                vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
            )),
        );

        // 导出渲染目标（用于后续呈现）
        rg_builder.export_image(
            present_image,
            RgImageState::PRESENT_BOTTOM,
            Some(RgSemaphoreInfo::binary(
                render_present.current_render_compute_semaphore().handle(),
                vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
            )),
        );

        // 添加 Pass
        rg_builder
            .add_pass(
                "resolve",
                ResolveRgPass {
                    resolve_pass: &self.resolve_pass,
                    render_world,
                    render_target,
                    swapchain_image: present_image,
                    swapchain_extent: render_present.swapchain_image_info().image_extent,
                },
            )
            .add_pass(
                "gui",
                GuiRgPass {
                    gui_pass: &self.gui_pass,
                    render_world,

                    ui_draw_data: gui_draw_data,
                    gui_mesh: &render_present.gui_backend.gui_meshes[*frame_label],
                    tex_map: &render_present.gui_backend.tex_map,

                    canvas_color: present_image,
                    canvas_extent: render_present.swapchain_image_info().image_extent,
                },
            );
    }
}

impl Drop for RtPipeline {
    fn drop(&mut self) {
        log::info!("RtRenderGraph drop");
    }
}
