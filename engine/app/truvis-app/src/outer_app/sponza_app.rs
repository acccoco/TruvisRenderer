use truvis_app_frame::input_event::InputEvent;
use truvis_app_frame::plugin_api::{Plugin, PluginRenderCtx};
use truvis_app_frame::render_app_api::{RenderAppHooks, RenderAppInitCtx};
use truvis_asset::handle::{AssetModelHandle, LoadStatus};
use truvis_path::TruvisPath;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgSemaphoreInfo};
use truvis_render_runtime::platform::camera::Camera;
use truvis_render_runtime::render_runtime::{RenderRuntimeRenderCtx, RenderRuntimeUpdateCtx};
use truvis_shader_binding::gpu;
use truvis_world::World;

use crate::camera_controller::CameraController;
use crate::gui_plugin::GuiPlugin;
use crate::input_state::InputManager;
use crate::overlay::{DebugInfoOverlay, PipelineControlsOverlay};
use crate::render_pipeline::rt_render_graph::RtPipeline;

#[derive(Default)]
pub struct SponzaApp {
    gui: GuiPlugin,
    rt_pipeline: RtPipeline,
    camera_controller: CameraController,
    input: InputManager,
    debug_overlay: DebugInfoOverlay,
    pipeline_overlay: PipelineControlsOverlay,
    model_asset: Option<AssetModelHandle>,
    model_spawned: bool,
}

impl SponzaApp {
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
}

impl RenderAppHooks for SponzaApp {
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

        let delta = std::time::Duration::from_secs_f32(ctx.delta_time_s);
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
