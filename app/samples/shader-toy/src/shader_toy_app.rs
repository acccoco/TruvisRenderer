use ash::vk;
use itertools::Itertools;

use truvis_app_frame::input_event::InputEvent;
use truvis_app_frame::plugin_api::{Plugin, PluginInitCtx, PluginRenderCtx, PluginShutdownCtx};
use truvis_app_frame::render_app_api::{RenderAppHooks, RenderAppInitCtx, RenderAppShutdownCtx};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_render_foundation::frame_counter::FrameCounter;
use truvis_render_foundation::render_view::RenderView;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle, RgImageState, RgSemaphoreInfo};
use truvis_render_runtime::render_runtime::{RenderRuntimeRenderCtx, RenderRuntimeUpdateCtx};

use crate::shader_toy_pass::ShaderToyPass;
use app_kit::camera_controller::CameraController;
use app_kit::gui_plugin::GuiPlugin;
use app_kit::input_state::InputManager;
use app_kit::overlay::{DebugInfoOverlay, PipelineControlsOverlay};

#[derive(Default)]
pub struct ShaderToyPlugin {
    shader_toy_pass: Option<ShaderToyPass>,
}

impl Plugin for ShaderToyPlugin {
    fn init(&mut self, ctx: &mut PluginInitCtx) {
        self.shader_toy_pass = Some(ShaderToyPass::new(ctx.device_ctx, ctx.swapchain_image_info.image_format));
    }

    fn shutdown(&mut self, ctx: &mut PluginShutdownCtx<'_>) {
        if let Some(pass) = self.shader_toy_pass.take() {
            pass.destroy(ctx.device_ctx);
        }
    }
}

impl ShaderToyPlugin {
    pub fn contribute_passes<'a>(
        &'a self,
        graph: &mut RenderGraphBuilder<'a>,
        ctx: &'a PluginRenderCtx<'a>,
        canvas_color: RgImageHandle,
        canvas_extent: vk::Extent2D,
    ) {
        graph.add_pass_lambda(
            "shader-toy",
            move |builder| {
                builder.read_write_image(canvas_color, RgImageState::COLOR_ATTACHMENT_READ_WRITE);
            },
            move |context| {
                let canvas_view = context.get_image_view(canvas_color).unwrap();
                self.shader_toy_pass.as_ref().expect("ShaderToyPlugin not initialized").draw(
                    ctx.gpu_store,
                    context.cmd,
                    canvas_view,
                    canvas_extent,
                );
            },
        );
    }
}

#[derive(Default)]
pub struct ShaderToy {
    gui: GuiPlugin,
    shader_toy: ShaderToyPlugin,
    camera_controller: CameraController,
    input: InputManager,
    debug_overlay: DebugInfoOverlay,
    pipeline_overlay: PipelineControlsOverlay,
    cmds: Vec<GfxCommandBuffer>,
}

impl RenderAppHooks for ShaderToy {
    fn init(&mut self, ctx: &mut RenderAppInitCtx<'_>) {
        self.gui.set_hidpi_factor(ctx.scale_factor);
        self.gui.set_display_size(ctx.window_size);

        let cmd_allocator = &mut *ctx.runtime.cmd_allocator;
        self.cmds = FrameCounter::frame_labes()
            .iter()
            .map(|label| cmd_allocator.alloc_command_buffer(ctx.runtime.device_ctx, *label, "shader-toy-app"))
            .collect_vec();
    }

    fn visit_plugins_mut(&mut self, visit: &mut dyn FnMut(&mut dyn Plugin)) {
        visit(&mut self.shader_toy);
        visit(&mut self.gui);
        visit(&mut self.debug_overlay);
        visit(&mut self.pipeline_overlay);
    }

    fn visit_plugins_mut_rev(&mut self, visit: &mut dyn FnMut(&mut dyn Plugin)) {
        visit(&mut self.pipeline_overlay);
        visit(&mut self.debug_overlay);
        visit(&mut self.shader_toy);
        visit(&mut self.gui);
    }

    fn on_input(&mut self, events: &[InputEvent]) {
        self.input.begin_frame();
        for event in events {
            if !self.gui.on_input(event) {
                self.input.process_event(event);
            }
        }
    }

    fn update(&mut self, ctx: &mut RenderRuntimeUpdateCtx) {
        let delta = std::time::Duration::from_secs_f32(ctx.delta_time_s);
        self.gui.begin_frame(delta);
        {
            let ui = self.gui.ui();
            ui.text_wrapped("Hello world!");
            ui.text_wrapped("こんにちは世界！");
            self.debug_overlay.build_overlay_ui(
                ui,
                self.camera_controller.camera(),
                ctx.swapchain_extent,
                ctx.accum_data.accum_frames_num(),
                ctx.delta_time_s,
            );
            self.pipeline_overlay.build_overlay_ui(ui, ctx.pipeline_settings);
        }
        self.gui.end_frame();

        self.camera_controller.update(
            self.input.state(),
            glam::vec2(ctx.swapchain_extent.width as f32, ctx.swapchain_extent.height as f32),
            delta,
        );
    }

    fn render(&mut self, ctx: &RenderRuntimeRenderCtx) {
        let plugin_ctx = PluginRenderCtx {
            device_ctx: ctx.device_ctx,
            resource_ctx: ctx.resource_ctx,
            queue_ctx: ctx.queue_ctx,
            device_info_ctx: ctx.device_info_ctx,
            gpu_store: ctx.gpu_store,
            render_scene: ctx.render_scene,
            render_present: ctx.render_present,
            timeline: ctx.timeline,
        };
        self.gui.prepare_render_data(&plugin_ctx);

        let frame_label = ctx.gpu_store.frame_counter.frame_label();
        let frame_id = ctx.gpu_store.frame_counter.frame_id();
        let render_present = ctx.render_present;
        let present_target = render_present.current_target(frame_label);
        let swapchain_extent = present_target.swapchain_image_info.image_extent;

        let mut graph = RenderGraphBuilder::new();
        graph.signal_semaphore(RgSemaphoreInfo::timeline(
            ctx.timeline.handle(),
            vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
            frame_id,
        ));

        let swapchain_image = graph.import_image(
            "swapchain-image",
            present_target.render_target_image_handle,
            Some(present_target.render_target_view_handle),
            present_target.swapchain_image_info.image_format,
            RgImageState::UNDEFINED_BOTTOM,
            Some(RgSemaphoreInfo::binary(
                present_target.present_complete_semaphore.handle(),
                vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
            )),
        );

        graph.export_image(
            swapchain_image,
            RgImageState::PRESENT_BOTTOM,
            Some(RgSemaphoreInfo::binary(
                present_target.render_complete_semaphore.handle(),
                vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
            )),
        );

        self.shader_toy.contribute_passes(&mut graph, &plugin_ctx, swapchain_image, swapchain_extent);
        self.gui.contribute_passes(&mut graph, &plugin_ctx, swapchain_image, swapchain_extent);

        let compiled_graph = graph.compile();
        if log::log_enabled!(log::Level::Debug) {
            static PRINT_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
            PRINT_DEBUG_INFO.call_once(|| {
                compiled_graph.print_execution_plan();
            });
        }

        let cmd = &self.cmds[*frame_label];
        cmd.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "shader-toy-graph");
        compiled_graph.execute(cmd, &ctx.gpu_store.gfx_resource_manager);
        cmd.end();

        let submit_info = compiled_graph.build_submit_info(std::slice::from_ref(cmd));
        ctx.queue_ctx.gfx_queue().submit(vec![submit_info], None);
    }

    fn render_view(&self) -> RenderView {
        self.camera_controller.camera().render_view()
    }

    fn shutdown(&mut self, _ctx: &mut RenderAppShutdownCtx<'_>) {
        self.cmds.clear();
    }
}
