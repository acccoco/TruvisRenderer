use truvis_app_frame::input_event::InputEvent;
use truvis_app_frame::plugin_api::{Plugin, PluginRenderCtx};
use truvis_app_frame::render_app_api::{RenderAppHooks, RenderAppInitCtx};
use truvis_asset::handle::{AssetModelHandle, LoadStatus};
use truvis_path::TruvisPath;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgSemaphoreInfo};
use truvis_render_runtime::platform::camera::Camera;
use truvis_render_runtime::ray_cast::{RayCastRay, RayCastResult};
use truvis_render_runtime::render_runtime::{RenderRuntimeRayCastCtx, RenderRuntimeRenderCtx, RenderRuntimeUpdateCtx};
use truvis_shader_binding::gpu;
use truvis_world::World;

use app_kit::camera_controller::CameraController;
use app_kit::gui_plugin::GuiPlugin;
use app_kit::input_state::InputManager;
use app_kit::overlay::{DebugInfoOverlay, PipelineControlsOverlay};
use app_kit::render_pipeline::rt_render_graph::RtPipeline;

#[derive(Default)]
pub struct TruvisApp {
    gui: GuiPlugin,
    rt_pipeline: RtPipeline,
    camera_controller: CameraController,
    input: InputManager,
    debug_overlay: DebugInfoOverlay,
    pipeline_overlay: PipelineControlsOverlay,
    click_ray_cast_probe: ClickRayCastProbe,
    model_asset: Option<AssetModelHandle>,
    model_spawned: bool,
}

struct ClickRayCastProbe {
    total_time_s: f32,
    pending_ray: Option<RayCastRay>,
    pending_screen_pos: Option<glam::Vec2>,
    last_screen_pos: Option<glam::Vec2>,
    last_result: Option<RayCastResult>,
    last_error: Option<String>,
    last_cast_time_s: Option<f32>,
}

impl Default for ClickRayCastProbe {
    fn default() -> Self {
        Self {
            total_time_s: 0.0,
            pending_ray: None,
            pending_screen_pos: None,
            last_screen_pos: None,
            last_result: None,
            last_error: None,
            last_cast_time_s: None,
        }
    }
}

impl ClickRayCastProbe {
    fn update_time(&mut self, delta_time_s: f32) {
        self.total_time_s += delta_time_s.max(0.0);
    }

    fn request_cast(&mut self, screen_pos: glam::Vec2, ray: Option<RayCastRay>) {
        self.last_screen_pos = Some(screen_pos);
        match ray {
            Some(ray) => {
                self.pending_ray = Some(ray);
                self.pending_screen_pos = Some(screen_pos);
                self.last_error = None;
            }
            None => {
                self.pending_ray = None;
                self.pending_screen_pos = None;
                self.last_result = None;
                self.last_error = Some("click position is outside the viewport".to_owned());
                self.last_cast_time_s = None;
            }
        }
    }

    fn take_pending_cast(&mut self) -> Option<(RayCastRay, glam::Vec2)> {
        let ray = self.pending_ray.take()?;
        let screen_pos = self.pending_screen_pos.take().expect("pending raycast missing screen position");
        Some((ray, screen_pos))
    }

    fn finish_cast(&mut self, screen_pos: glam::Vec2, result: Result<RayCastResult, String>) {
        match result {
            Ok(result) => {
                self.last_result = Some(result);
                self.last_error = None;
            }
            Err(err) => {
                self.last_result = None;
                self.last_error = Some(err);
            }
        }
        self.last_screen_pos = Some(screen_pos);
        self.last_cast_time_s = Some(self.total_time_s);
    }

    fn has_pending_cast(&self) -> bool {
        self.pending_ray.is_some()
    }
}

impl TruvisApp {
    fn request_model(world: &mut World, camera: &mut Camera) -> AssetModelHandle {
        camera.position = glam::vec3(270.0, 194.0, -64.0);
        camera.euler_yaw_deg = 90.0;
        camera.euler_pitch_deg = 0.0;

        world.scene_manager.register_point_light(gpu::PointLight {
            pos: glam::vec3(-20.0, 40.0, 0.0).into(),
            color: (glam::vec3(5.0, 6.0, 1.0) * 2.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        world.scene_manager.register_point_light(gpu::PointLight {
            pos: glam::vec3(40.0, 40.0, -30.0).into(),
            color: (glam::vec3(1.0, 6.0, 7.0) * 3.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        world.scene_manager.register_point_light(gpu::PointLight {
            pos: glam::vec3(40.0, 40.0, 30.0).into(),
            color: (glam::vec3(5.0, 1.0, 8.0) * 3.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });

        log::info!("start load sponza model");
        world.asset_hub.load_model(TruvisPath::assets_path("fbx/sponza/sponza.fbx"))
    }

    fn spawn_model_if_ready(&mut self, world: &mut World) {
        if self.model_spawned {
            return;
        }

        let Some(model_asset) = self.model_asset else {
            return;
        };

        match world.asset_hub.get_model_status(model_asset) {
            LoadStatus::Ready => {
                let model_data = world.asset_hub.get_model_data(model_asset).expect("ready model asset missing data");
                let instances = world.scene_manager.spawn_model(model_data);
                self.model_spawned = true;
                log::info!("Sponza model spawned {} runtime instances.", instances.len());
            }
            LoadStatus::Failed => {
                self.model_spawned = true;
                let error = world.asset_hub.get_model_error(model_asset).unwrap_or("unknown error");
                log::error!("Sponza model failed to load: {}", error);
            }
            LoadStatus::Unloaded | LoadStatus::Loading => {}
        }
    }

    fn build_raycast_overlay_ui(&self, ui: &imgui::Ui) {
        ui.window("Raycast")
            .position([10.0, 420.0], imgui::Condition::FirstUseEver)
            .size([340.0, 250.0], imgui::Condition::FirstUseEver)
            .build(|| {
                ui.text("Trigger: left mouse click");
                if self.click_ray_cast_probe.has_pending_cast() {
                    ui.text("Status: pending");
                } else {
                    ui.text("Status: idle");
                }

                if let Some(screen_pos) = self.click_ray_cast_probe.last_screen_pos {
                    ui.text(format!("Last click: ({:.0}, {:.0})", screen_pos.x, screen_pos.y));
                } else {
                    ui.text("Last click: never");
                }

                if let Some(last_cast_time_s) = self.click_ray_cast_probe.last_cast_time_s {
                    ui.text(format!("Last cast at: {:.2}s", last_cast_time_s));
                } else {
                    ui.text("Last cast: never");
                }
                ui.separator();

                if let Some(error) = &self.click_ray_cast_probe.last_error {
                    ui.text(format!("Error: {error}"));
                    return;
                }

                match &self.click_ray_cast_probe.last_result {
                    Some(RayCastResult::Miss) => {
                        ui.text("Result: Miss");
                    }
                    Some(RayCastResult::Hit(hit)) => {
                        ui.text("Result: Hit");
                        ui.text(format!("Instance: {:?}", hit.instance));
                        ui.text(format!("Mesh: {:?}", hit.mesh));
                        ui.text(format!("Material: {:?}", hit.material));
                        ui.text(format!("Submesh: {}", hit.submesh_index));
                        ui.text(format!("Primitive: {}", hit.primitive_index));
                        ui.text(format!("Hit T: {:.3}", hit.hit_t));
                        ui.text(format!(
                            "Position: ({:.2}, {:.2}, {:.2})",
                            hit.position_ws.x, hit.position_ws.y, hit.position_ws.z
                        ));
                        ui.text(format!(
                            "Normal: ({:.2}, {:.2}, {:.2})",
                            hit.normal_ws.x, hit.normal_ws.y, hit.normal_ws.z
                        ));
                        ui.text(format!("UV: ({:.3}, {:.3})", hit.uv.x, hit.uv.y));
                    }
                    None => {
                        ui.text("Result: waiting");
                    }
                }
            });
    }

    fn cast_single_ray(ctx: &mut RenderRuntimeRayCastCtx<'_>, ray: RayCastRay) -> Result<RayCastResult, String> {
        ctx.cast_sync(std::slice::from_ref(&ray))
            .map_err(|err| err.to_string())
            .map(|mut results| results.pop().expect("single raycast result missing"))
    }
}

impl RenderAppHooks for TruvisApp {
    fn init(&mut self, ctx: &mut RenderAppInitCtx<'_>) {
        self.gui.set_hidpi_factor(ctx.scale_factor);
        self.gui.set_display_size(ctx.window_size);

        self.model_asset = Some(Self::request_model(&mut *ctx.runtime.world, self.camera_controller.camera_mut()));
    }

    fn visit_plugins_mut(&mut self, visit: &mut dyn FnMut(&mut dyn Plugin)) {
        visit(&mut self.rt_pipeline);
        visit(&mut self.gui);
        visit(&mut self.debug_overlay);
        visit(&mut self.pipeline_overlay);
    }

    fn visit_plugins_mut_rev(&mut self, visit: &mut dyn FnMut(&mut dyn Plugin)) {
        visit(&mut self.pipeline_overlay);
        visit(&mut self.debug_overlay);
        visit(&mut self.rt_pipeline);
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
        self.spawn_model_if_ready(ctx.world);
        self.click_ray_cast_probe.update_time(ctx.delta_time_s);

        let delta = std::time::Duration::from_secs_f32(ctx.delta_time_s);
        let viewport_size = glam::vec2(ctx.swapchain_extent.width as f32, ctx.swapchain_extent.height as f32);
        self.camera_controller.update_with_wheel_zoom(self.input.state(), viewport_size, delta);

        if self.input.state().is_left_button_just_pressed() {
            let mouse_position = self.input.state().mouse_position();
            let screen_pos = glam::vec2(mouse_position[0] as f32, mouse_position[1] as f32);
            let ray = self.camera_controller.make_screen_raycast(mouse_position, viewport_size);
            self.click_ray_cast_probe.request_cast(screen_pos, ray);
        }

        self.gui.begin_frame(delta);
        {
            let ui = self.gui.ui();
            self.debug_overlay.build_overlay_ui(
                ui,
                self.camera_controller.camera(),
                ctx.swapchain_extent,
                ctx.accum_data.accum_frames_num(),
                ctx.delta_time_s,
            );
            self.pipeline_overlay.build_overlay_ui(ui, ctx.pipeline_settings);
            self.build_raycast_overlay_ui(ui);
        }
        self.gui.end_frame();
    }

    fn after_prepare(&mut self, ctx: &mut RenderRuntimeRayCastCtx<'_>) {
        if let Some(request) = self.camera_controller.take_pending_pivot_raycast() {
            let result = Self::cast_single_ray(ctx, request.ray);
            self.camera_controller.finish_pivot_raycast(request, result);
        }

        if let Some(request) = self.camera_controller.take_pending_drag_pan_raycast() {
            let result = Self::cast_single_ray(ctx, request.ray);
            self.camera_controller.finish_drag_pan_raycast(request, result);
        }

        if let Some(request) = self.camera_controller.take_pending_wheel_zoom_raycast() {
            let result = Self::cast_single_ray(ctx, request.ray);
            self.camera_controller.finish_wheel_zoom_raycast(request, result);
        }

        if let Some((ray, screen_pos)) = self.click_ray_cast_probe.take_pending_cast() {
            let result = Self::cast_single_ray(ctx, ray);
            self.click_ray_cast_probe.finish_cast(screen_pos, result);
        }
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

        let compute_submit = {
            let mut graph = RenderGraphBuilder::new();
            self.rt_pipeline.contribute_compute_passes(&mut graph, &plugin_ctx);
            let compiled_graph = graph.compile();
            if log::log_enabled!(log::Level::Debug) {
                static PRINT_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
                PRINT_DEBUG_INFO.call_once(|| {
                    compiled_graph.print_execution_plan();
                });
            }

            let cmd = self.rt_pipeline.compute_cmd(frame_label);
            cmd.begin(ash::vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "rt-compute-graph");
            compiled_graph.execute(cmd, &ctx.gpu_store.gfx_resource_manager);
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
            let present_image = self.rt_pipeline.contribute_present_passes(&mut graph, &plugin_ctx);
            self.gui.contribute_passes(
                &mut graph,
                &plugin_ctx,
                present_image,
                ctx.render_present.swapchain_image_info().image_extent,
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
            compiled_graph.execute(cmd, &ctx.gpu_store.gfx_resource_manager);
            cmd.end();
            compiled_graph.build_submit_info(std::slice::from_ref(cmd))
        };

        ctx.queue_ctx.gfx_queue().submit(vec![compute_submit, present_submit], None);
    }

    fn camera(&self) -> &Camera {
        self.camera_controller.camera()
    }
}
