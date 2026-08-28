use ash::vk;
use itertools::Itertools;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_render_foundation::render_view::RenderView;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgSemaphoreInfo};
use truvis_render_loop::input_event::InputEvent;
use truvis_render_loop::renderer::{Renderer, RendererInitCtx, RendererResizeCtx, RendererShutdownCtx};
use truvis_render_runtime::render_runtime::{RenderRuntimeRenderCtx, RenderRuntimeUpdateCtx};

use renderer_imgui::{DebugInfoOverlay, ImGuiSubsystem};
use renderer_kit::camera_controller::CameraController;
use renderer_kit::input_state::InputManager;
use renderer_kit::subsystem::{SubsystemLifecycle, SubsystemRenderCtx};

use crate::triangle_subsystem::TriangleSubsystem;

#[derive(Default)]
pub struct TriangleRenderer {
    imgui: ImGuiSubsystem,
    triangle: TriangleSubsystem,
    camera_controller: CameraController,
    input: InputManager,
    debug_overlay: DebugInfoOverlay,
    cmds: Vec<GfxCommandBuffer>,
}

impl Renderer for TriangleRenderer {
    fn init(&mut self, ctx: &mut RendererInitCtx<'_>) {
        self.imgui.set_hidpi_factor(ctx.scale_factor);
        self.imgui.set_display_size(ctx.window_size);

        let cmd_allocator = &mut *ctx.runtime.cmd_allocator;
        self.cmds = FrameLabel::ALL
            .iter()
            .map(|label| cmd_allocator.alloc_command_buffer(ctx.runtime.device_ctx, *label, "triangle-app"))
            .collect_vec();

        self.triangle.init(&mut ctx.runtime);
        self.imgui.init(&mut ctx.runtime);
    }

    fn on_input(&mut self, events: &[InputEvent]) {
        self.input.begin_frame();
        for event in events {
            if !self.imgui.on_input(event) {
                self.input.process_event(event);
            }
        }
    }

    fn update(&mut self, ctx: &mut RenderRuntimeUpdateCtx) {
        let delta = std::time::Duration::from_secs_f32(ctx.frame_timing.delta_time_s());
        self.imgui.build_frame(delta, |ui| {
            self.debug_overlay.build_overlay_ui(
                ui,
                self.camera_controller.camera(),
                ctx.swapchain_extent,
                ctx.view_accum.accum_frames_num(),
            );
        });

        self.camera_controller.update(
            self.input.state(),
            glam::vec2(ctx.swapchain_extent.width as f32, ctx.swapchain_extent.height as f32),
            delta,
        );
    }

    fn render(&mut self, ctx: &RenderRuntimeRenderCtx) {
        let subsystem_ctx = SubsystemRenderCtx::from_runtime(ctx);
        self.imgui.prepare_render_data(&subsystem_ctx);

        let frame_label = ctx.record_ctx.frame_timing.frame_label();
        let frame_id = ctx.record_ctx.frame_timing.frame_id();

        let mut graph = RenderGraphBuilder::new();
        graph.signal_semaphore(RgSemaphoreInfo::timeline(
            ctx.timeline.handle(),
            vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
            frame_id,
        ));
        let present_target = ctx.present.import_current_target(&mut graph, frame_label);
        let swapchain_image = present_target.image;
        let swapchain_extent = present_target.image_info.image_extent;

        self.triangle.contribute_passes(&mut graph, swapchain_image, swapchain_extent);
        self.imgui.contribute_passes(&mut graph, &subsystem_ctx, swapchain_image, swapchain_extent);

        let compiled_graph = graph.compile();
        if log::log_enabled!(log::Level::Debug) {
            static PRINT_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
            PRINT_DEBUG_INFO.call_once(|| {
                compiled_graph.print_execution_plan();
            });
        }

        let cmd = &self.cmds[*frame_label];
        cmd.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "triangle-graph");
        compiled_graph.execute(cmd, ctx.record_ctx.gfx_resource_manager);
        cmd.end();

        let submit_info = compiled_graph.build_submit_info(std::slice::from_ref(cmd));
        ctx.queue_ctx.gfx_queue().submit(vec![submit_info], None);
    }

    fn render_view(&self) -> RenderView {
        self.camera_controller.camera().render_view()
    }

    fn on_resize(&mut self, ctx: &mut RendererResizeCtx<'_>) {
        self.triangle.on_resize(&mut ctx.runtime);
        self.imgui.on_resize(&mut ctx.runtime);
    }

    fn shutdown(&mut self, ctx: &mut RendererShutdownCtx<'_>) {
        self.cmds.clear();
        self.imgui.shutdown(&mut ctx.runtime);
        self.triangle.shutdown(&mut ctx.runtime);
    }
}
