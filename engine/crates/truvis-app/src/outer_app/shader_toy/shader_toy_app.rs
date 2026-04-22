use ash::vk;
use itertools::Itertools;

use truvis_app_api::app_plugin::{AppPlugin, InitCtx, RenderCtx, UpdateCtx};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::Gfx;
use truvis_gui_backend::gui_pass::GuiPass;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageState, RgSemaphoreInfo};
use truvis_render_interface::frame_counter::FrameCounter;

use crate::gui_rg_pass::GuiRgPass;
use crate::outer_app::shader_toy::shader_toy_pass::ShaderToyPass;

#[derive(Default)]
pub struct ShaderToy {
    shader_toy_pass: Option<ShaderToyPass>,
    gui_pass: Option<GuiPass>,
    cmds: Vec<GfxCommandBuffer>,
}

impl AppPlugin for ShaderToy {
    fn init(&mut self, ctx: &mut InitCtx) {
        log::info!("shader toy.");

        self.shader_toy_pass = Some(ShaderToyPass::new(ctx.swapchain_image_info.image_format));
        self.gui_pass = Some(GuiPass::new(ctx.global_descriptor_sets, ctx.swapchain_image_info.image_format));

        self.cmds = FrameCounter::frame_labes()
            .iter()
            .map(|label| ctx.cmd_allocator.alloc_command_buffer(*label, "triangle-app"))
            .collect_vec();
    }

    fn build_ui(&mut self, ui: &imgui::Ui) {
        ui.text_wrapped("Hello world!");
        ui.text_wrapped("こんにちは世界！");
    }
    fn update(&mut self, _ctx: &mut UpdateCtx) {}

    fn render(&self, ctx: &RenderCtx) {
        let frame_label = ctx.render_context.frame_counter.frame_label();
        let frame_id = ctx.render_context.frame_counter.frame_id();
        let render_present = ctx.render_present;

        let (swapchain_image_handle, swapchain_view_handle) = render_present.current_image_and_view();

        let mut graph = RenderGraphBuilder::new();
        graph.signal_semaphore(RgSemaphoreInfo::timeline(
            ctx.timeline.handle(),
            vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
            frame_id,
        ));

        let swapchain_image_rg_handle = graph.import_image(
            "swapchain-image",
            swapchain_image_handle,
            Some(swapchain_view_handle),
            render_present.swapchain_image_info().image_format,
            RgImageState::UNDEFINED_BOTTOM,
            Some(RgSemaphoreInfo::binary(
                render_present.current_present_complete_semaphore(frame_label).handle(),
                vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
            )),
        );

        graph.export_image(
            swapchain_image_rg_handle,
            RgImageState::PRESENT_BOTTOM,
            Some(RgSemaphoreInfo::binary(
                render_present.current_render_compute_semaphore().handle(),
                vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
            )),
        );

        graph
            .add_pass_lambda(
                "shader-toy",
                |builder| {
                    builder.read_write_image(swapchain_image_rg_handle, RgImageState::COLOR_ATTACHMENT_READ_WRITE);
                },
                |context| {
                    let canvas_view = context.get_image_view(swapchain_image_rg_handle).unwrap();
                    self.shader_toy_pass.as_ref().unwrap().draw(
                        ctx.render_context,
                        context.cmd,
                        canvas_view,
                        render_present.swapchain_image_info().image_extent,
                    );
                },
            )
            .add_pass(
                "gui",
                GuiRgPass {
                    gui_pass: self.gui_pass.as_ref().unwrap(),
                    render_context: ctx.render_context,

                    ui_draw_data: ctx.gui_draw_data,
                    gui_mesh: &render_present.gui_backend.gui_meshes[*frame_label],
                    tex_map: &render_present.gui_backend.tex_map,

                    canvas_color: swapchain_image_rg_handle,
                    canvas_extent: render_present.swapchain_image_info().image_extent,
                },
            );

        let compiled_graph = graph.compile();

        if log::log_enabled!(log::Level::Debug) {
            static PRINT_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
            PRINT_DEBUG_INFO.call_once(|| {
                compiled_graph.print_execution_plan();
            });
        }

        let cmd = &self.cmds[*frame_label];
        cmd.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "rt-present-graph");
        compiled_graph.execute(cmd, &ctx.render_context.gfx_resource_manager);
        cmd.end();

        let submit_info = compiled_graph.build_submit_info(std::slice::from_ref(cmd));

        Gfx::get().gfx_queue().submit(vec![submit_info], None);
    }
}
