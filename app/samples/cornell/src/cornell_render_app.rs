use truvis_app_frame::input_event::InputEvent;
use truvis_app_frame::render_app_api::{RenderApp, RenderAppInitCtx, RenderAppResizeCtx, RenderAppShutdownCtx};
use truvis_path::TruvisPath;
use truvis_render_foundation::render_view::RenderView;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgSemaphoreInfo};
use truvis_render_runtime::render_runtime::{RenderRuntimeRenderCtx, RenderRuntimeUpdateCtx};
use truvis_shader_binding::gpu;
use truvis_world::World;

use app_kit::camera::Camera;
use app_kit::camera_controller::CameraController;
use app_kit::debug_image::DebugImageSelector;
use app_kit::gui_subsystem::GuiSubsystem;
use app_kit::input_state::InputManager;
use app_kit::overlay::{DebugInfoOverlay, PipelineControlsOverlay};
use app_kit::render_pipeline::RenderMode;
use app_kit::render_pipeline::common_settings::PathTracingCommonSettings;
use app_kit::render_pipeline::rt_render_graph::RtPipeline;
use app_kit::subsystem::{SubsystemLifecycle, SubsystemRenderCtx};

#[derive(Default)]
pub struct CornellRenderApp {
    gui: GuiSubsystem,
    debug_image_selector: DebugImageSelector,
    rt_pipeline: RtPipeline,
    path_tracing_common_settings: PathTracingCommonSettings,
    camera_controller: CameraController,
    input: InputManager,
    debug_overlay: DebugInfoOverlay,
    pipeline_overlay: PipelineControlsOverlay,
}

impl CornellRenderApp {
    fn request_model(world: &mut World, camera: &mut Camera) {
        camera.position = glam::vec3(-400.0, 1000.0, 1000.0);
        camera.euler_yaw_deg = 330.0;
        camera.euler_pitch_deg = -27.0;

        world.register_point_light(gpu::engine::light::PointLight {
            pos: glam::vec3(-20.0, 40.0, 0.0).into(),
            color: (glam::vec3(5.0, 6.0, 1.0) * 2.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        world.register_point_light(gpu::engine::light::PointLight {
            pos: glam::vec3(40.0, 40.0, -30.0).into(),
            color: (glam::vec3(1.0, 6.0, 7.0) * 3.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        world.register_point_light(gpu::engine::light::PointLight {
            pos: glam::vec3(40.0, 40.0, 30.0).into(),
            color: (glam::vec3(5.0, 1.0, 8.0) * 3.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        world.register_spot_light(gpu::engine::light::SpotLight {
            pos: glam::vec3(0.0, 320.0, 180.0).into(),
            inner_angle: 12.0_f32.to_radians(),
            color: (glam::vec3(8.0, 6.0, 3.0) * 8.0).into(),
            outer_angle: 28.0_f32.to_radians(),
            dir: glam::vec3(0.0, -0.85, -0.35).normalize().into(),
            _dir_padding: Default::default(),
        });
        world.register_area_light(gpu::engine::light::AreaLight {
            center: glam::vec3(0.0, 380.0, 0.0).into(),
            half_u: glam::vec3(80.0, 0.0, 0.0).into(),
            half_v: glam::vec3(0.0, 0.0, 80.0).into(),
            radiance: (glam::vec3(1.0, 0.92, 0.75) * 2.0).into(),
            _center_padding: Default::default(),
            _half_u_padding: Default::default(),
            _half_v_padding: Default::default(),
            _radiance_padding: Default::default(),
        });

        log::info!("Loading model...");
        world.request_model_import(TruvisPath::assets_path("fbx/cornell-box.fbx"));
    }
}

impl RenderApp for CornellRenderApp {
    fn init(&mut self, ctx: &mut RenderAppInitCtx<'_>) {
        self.gui.set_hidpi_factor(ctx.scale_factor);
        self.gui.set_display_size(ctx.window_size);

        Self::request_model(&mut *ctx.runtime.world, self.camera_controller.camera_mut());

        self.rt_pipeline.init(&mut ctx.runtime);
        self.gui.init(&mut ctx.runtime);
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
        let delta = std::time::Duration::from_secs_f32(ctx.frame_timing.delta_time_s());
        self.gui.build_frame(delta, |ui| {
            self.debug_overlay.build_overlay_ui(
                ui,
                self.camera_controller.camera(),
                ctx.swapchain_extent,
                ctx.view_accum.accum_frames_num(),
                ctx.frame_timing.delta_time_s(),
            );
            // Sample app 不持有 OfflinePipeline；临时 Realtime 只用于复用共享 Controls overlay 的签名。
            let mut render_mode = RenderMode::Realtime;
            self.pipeline_overlay.build_overlay_ui(
                ui,
                &mut render_mode,
                ctx.dlss_options,
                Some(&mut self.path_tracing_common_settings),
                Some(self.rt_pipeline.settings_mut()),
                None,
                None,
            );
            self.debug_image_selector.build_window(ui, RtPipeline::debug_image_options());
        });

        self.camera_controller.update(
            self.input.state(),
            glam::vec2(ctx.swapchain_extent.width as f32, ctx.swapchain_extent.height as f32),
            delta,
        );
    }

    fn render(&mut self, ctx: &RenderRuntimeRenderCtx) {
        let subsystem_ctx = SubsystemRenderCtx::from_runtime(ctx);
        let frame_label = ctx.record_ctx.frame_timing.frame_label();
        let frame_id = ctx.record_ctx.frame_timing.frame_id();

        self.gui.prepare_render_data(&subsystem_ctx);

        let compute_submit = {
            let mut graph = RenderGraphBuilder::new();
            self.rt_pipeline.contribute_compute_passes(&mut graph, &subsystem_ctx, &self.path_tracing_common_settings);
            let compiled_graph = graph.compile();
            if log::log_enabled!(log::Level::Debug) {
                static PRINT_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
                PRINT_DEBUG_INFO.call_once(|| {
                    compiled_graph.print_execution_plan();
                });
            }

            let cmd = self.rt_pipeline.compute_cmd(frame_label);
            cmd.begin(ash::vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "rt-compute-graph");
            compiled_graph.execute(cmd, ctx.record_ctx.gfx_resource_manager);
            cmd.end();
            compiled_graph.build_submit_info(std::slice::from_ref(cmd))
        };

        let present_submit = {
            let mut graph = RenderGraphBuilder::new();
            graph.signal_semaphore(RgSemaphoreInfo::timeline(
                ctx.timeline.handle(),
                ash::vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
                frame_id,
            ));
            let present_targets = self.rt_pipeline.contribute_present_passes(
                &mut graph,
                &subsystem_ctx,
                &self.path_tracing_common_settings,
                self.debug_image_selector.selected_id(),
            );
            self.gui.contribute_passes(
                &mut graph,
                &subsystem_ctx,
                present_targets.present_image,
                ctx.present.swapchain_image_info().image_extent,
            );

            let compiled_graph = graph.compile();
            if log::log_enabled!(log::Level::Debug) {
                static PRINT_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
                PRINT_DEBUG_INFO.call_once(|| {
                    compiled_graph.print_execution_plan();
                });
            }

            let cmd = self.rt_pipeline.present_cmd(frame_label);
            cmd.begin(ash::vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "rt-present-graph");
            compiled_graph.execute(cmd, ctx.record_ctx.gfx_resource_manager);
            cmd.end();
            compiled_graph.build_submit_info(std::slice::from_ref(cmd))
        };

        ctx.queue_ctx.gfx_queue().submit(vec![compute_submit, present_submit], None);
    }

    fn render_view(&self) -> RenderView {
        self.camera_controller.camera().render_view()
    }

    fn on_resize(&mut self, ctx: &mut RenderAppResizeCtx<'_>) {
        self.rt_pipeline.on_resize(&mut ctx.runtime);
        self.gui.on_resize(&mut ctx.runtime);
    }

    fn shutdown(&mut self, ctx: &mut RenderAppShutdownCtx<'_>) {
        self.rt_pipeline.shutdown(&mut ctx.runtime);
        self.gui.shutdown(&mut ctx.runtime);
    }
}
