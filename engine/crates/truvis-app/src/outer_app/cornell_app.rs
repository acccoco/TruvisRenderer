use truvis_frame_api::frame_app::FrameAppHooks;
use truvis_frame_api::input_event::InputEvent;
use truvis_frame_api::plugin::{Plugin, PluginInitCtx, PluginRenderCtx, PluginResizeCtx};
use truvis_frame_runtime::{FrameAppInitCtx, FrameAppResizeCtx, FrameAppState};
use truvis_gfx::gfx::Gfx;
use truvis_path::TruvisPath;
use truvis_render_backend::model_loader::assimp_loader::AssimpSceneLoader;
use truvis_render_backend::platform::camera::Camera;
use truvis_render_backend::render_backend::{RenderBackendRenderCtx, RenderBackendUpdateCtx};
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgSemaphoreInfo};
use truvis_shader_binding::gpu;
use truvis_world::World;

use crate::camera_controller::CameraController;
use crate::gui_plugin::GuiPlugin;
use crate::input_state::InputManager;
use crate::overlay::{DebugInfoOverlay, PipelineControlsOverlay};
use crate::render_pipeline::rt_render_graph::RtPipeline;

#[derive(Default)]
pub struct CornellApp {
    gui: GuiPlugin,
    rt_pipeline: RtPipeline,
    camera_controller: CameraController,
    input: InputManager,
    debug_overlay: DebugInfoOverlay,
    pipeline_overlay: PipelineControlsOverlay,
}

impl CornellApp {
    fn create_scene(world: &mut World, camera: &mut Camera) {
        camera.position = glam::vec3(-400.0, 1000.0, 1000.0);
        camera.euler_yaw_deg = 330.0;
        camera.euler_pitch_deg = -27.0;

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

        log::info!("Loading scene...");
        AssimpSceneLoader::load_scene(
            TruvisPath::assets_path_str("fbx/cornell-box.fbx").as_ref(),
            &mut world.scene_manager,
            &mut world.asset_hub,
        );
        log::info!("Scene loaded.");
    }
}

impl FrameAppState for CornellApp {
    fn init(&mut self, ctx: FrameAppInitCtx<'_>) {
        let FrameAppInitCtx {
            backend: ctx,
            scale_factor,
            window_size,
        } = ctx;

        self.gui.set_hidpi_factor(scale_factor);
        self.gui.set_display_size(window_size);

        Self::create_scene(ctx.world, self.camera_controller.camera_mut());

        let mut plugin_ctx = PluginInitCtx {
            world: ctx.world,
            render_world: ctx.render_world,
            cmd_allocator: ctx.cmd_allocator,
            swapchain_image_info: ctx.swapchain_image_info,
            render_present: ctx.render_present,
        };
        self.rt_pipeline.init(&mut plugin_ctx);
        self.gui.init(&mut plugin_ctx);
        self.debug_overlay.init(&mut plugin_ctx);
        self.pipeline_overlay.init(&mut plugin_ctx);
    }

    fn on_resize(&mut self, ctx: FrameAppResizeCtx<'_>) {
        let ctx = ctx.backend;

        let mut plugin_ctx = PluginResizeCtx {
            render_world: ctx.render_world,
            render_present: ctx.render_present,
        };
        self.gui.on_resize(&mut plugin_ctx);
        self.rt_pipeline.on_resize(&mut plugin_ctx);
    }

    fn shutdown(&mut self) {
        self.pipeline_overlay.shutdown();
        self.debug_overlay.shutdown();
        self.rt_pipeline.shutdown();
        self.gui.shutdown();
    }
}

impl FrameAppHooks for CornellApp {
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
            compiled_graph.execute(cmd, &ctx.render_world.gfx_resource_manager);
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
            compiled_graph.execute(cmd, &ctx.render_world.gfx_resource_manager);
            cmd.end();
            compiled_graph.build_submit_info(std::slice::from_ref(cmd))
        };

        Gfx::get().gfx_queue().submit(vec![compute_submit, present_submit], None);
    }

    fn camera(&self) -> &Camera {
        self.camera_controller.camera()
    }
}
