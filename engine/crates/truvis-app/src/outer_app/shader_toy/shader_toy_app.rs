use ash::vk;
use itertools::Itertools;

use truvis_frame_api::input_event::InputEvent;
use truvis_frame_api::plugin::{Plugin, PluginInitCtx, PluginRenderCtx, PluginResizeCtx};
use truvis_frame_api::render_app::{RenderAppHooks, RenderAppInitCtx, RenderAppResizeCtx};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::Gfx;
use truvis_render_backend::platform::camera::Camera;
use truvis_render_backend::render_backend::{RenderBackendRenderCtx, RenderBackendUpdateCtx};
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle, RgImageState, RgSemaphoreInfo};
use truvis_render_interface::frame_counter::FrameCounter;

use crate::camera_controller::CameraController;
use crate::gui_plugin::GuiPlugin;
use crate::input_state::InputManager;
use crate::outer_app::shader_toy::shader_toy_pass::ShaderToyPass;
use crate::overlay::{DebugInfoOverlay, PipelineControlsOverlay};

#[derive(Default)]
pub struct ShaderToyPlugin {
    shader_toy_pass: Option<ShaderToyPass>,
}

impl Plugin for ShaderToyPlugin {
    fn init(&mut self, ctx: &mut PluginInitCtx) {
        self.shader_toy_pass = Some(ShaderToyPass::new(ctx.swapchain_image_info.image_format));
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
                    ctx.render_world,
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
    fn init(&mut self, ctx: RenderAppInitCtx<'_>) {
        let RenderAppInitCtx {
            backend: ctx,
            scale_factor,
            window_size,
        } = ctx;

        self.gui.set_hidpi_factor(scale_factor);
        self.gui.set_display_size(window_size);

        self.cmds = FrameCounter::frame_labes()
            .iter()
            .map(|label| ctx.cmd_allocator.alloc_command_buffer(*label, "shader-toy-app"))
            .collect_vec();

        let mut plugin_ctx = PluginInitCtx {
            world: ctx.world,
            render_world: ctx.render_world,
            cmd_allocator: ctx.cmd_allocator,
            swapchain_image_info: ctx.swapchain_image_info,
            render_present: ctx.render_present,
        };
        self.shader_toy.init(&mut plugin_ctx);
        self.gui.init(&mut plugin_ctx);
        self.debug_overlay.init(&mut plugin_ctx);
        self.pipeline_overlay.init(&mut plugin_ctx);
    }

    fn on_resize(&mut self, ctx: RenderAppResizeCtx<'_>) {
        let ctx = ctx.backend;

        let mut plugin_ctx = PluginResizeCtx {
            render_world: ctx.render_world,
            render_present: ctx.render_present,
        };
        self.gui.on_resize(&mut plugin_ctx);
        self.shader_toy.on_resize(&mut plugin_ctx);
    }

    fn shutdown(&mut self) {
        self.pipeline_overlay.shutdown();
        self.debug_overlay.shutdown();
        self.shader_toy.shutdown();
        self.gui.shutdown();
    }

    fn on_input(&mut self, events: &[InputEvent]) {
        self.input.begin_frame();
        for event in events {
            if !self.gui.on_input(event) {
                self.input.process_event(event);
            }
        }
    }

    fn update(&mut self, ctx: &mut RenderBackendUpdateCtx) {
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

    fn render(&mut self, ctx: &RenderBackendRenderCtx) {
        let plugin_ctx = PluginRenderCtx {
            render_world: ctx.render_world,
            render_present: ctx.render_present,
            timeline: ctx.timeline,
        };
        self.gui.prepare_render_data(&plugin_ctx);

        let frame_label = ctx.render_world.frame_counter.frame_label();
        let frame_id = ctx.render_world.frame_counter.frame_id();
        let render_present = ctx.render_present;
        let (swapchain_image_handle, swapchain_view_handle) = render_present.current_image_and_view();

        let mut graph = RenderGraphBuilder::new();
        graph.signal_semaphore(RgSemaphoreInfo::timeline(
            ctx.timeline.handle(),
            vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
            frame_id,
        ));

        let swapchain_image = graph.import_image(
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
            swapchain_image,
            RgImageState::PRESENT_BOTTOM,
            Some(RgSemaphoreInfo::binary(
                render_present.current_render_compute_semaphore().handle(),
                vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
            )),
        );

        self.shader_toy.contribute_passes(
            &mut graph,
            &plugin_ctx,
            swapchain_image,
            render_present.swapchain_image_info().image_extent,
        );
        self.gui.contribute_passes(
            &mut graph,
            &plugin_ctx,
            swapchain_image,
            render_present.swapchain_image_info().image_extent,
        );

        let compiled_graph = graph.compile();
        if log::log_enabled!(log::Level::Debug) {
            static PRINT_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
            PRINT_DEBUG_INFO.call_once(|| {
                compiled_graph.print_execution_plan();
            });
        }

        let cmd = &self.cmds[*frame_label];
        cmd.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "shader-toy-graph");
        compiled_graph.execute(cmd, &ctx.render_world.gfx_resource_manager);
        cmd.end();

        let submit_info = compiled_graph.build_submit_info(std::slice::from_ref(cmd));
        Gfx::get().gfx_queue().submit(vec![submit_info], None);
    }

    fn camera(&self) -> &Camera {
        self.camera_controller.camera()
    }
}
