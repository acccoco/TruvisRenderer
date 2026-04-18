use crate::app_plugin::AppPlugin;
use crate::render_pipeline::rt_render_graph::RtPipeline;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_path::TruvisPath;
use truvis_renderer::model_loader::assimp_loader::AssimpSceneLoader;
use truvis_renderer::platform::camera::Camera;
use truvis_renderer::renderer::Renderer;
use truvis_shader_binding::gpu;

#[derive(Default)]
pub struct CornellApp {
    rt_pipeline: Option<RtPipeline>,
}

impl CornellApp {
    fn create_scene(renderer: &mut Renderer, camera: &mut Camera) {
        camera.position = glam::vec3(-400.0, 1000.0, 1000.0);
        camera.euler_yaw_deg = 330.0;
        camera.euler_pitch_deg = -27.0;

        renderer.render_context.scene_manager.register_point_light(gpu::PointLight {
            pos: glam::vec3(-20.0, 40.0, 0.0).into(),
            color: (glam::vec3(5.0, 6.0, 1.0) * 2.0).into(),

            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        renderer.render_context.scene_manager.register_point_light(gpu::PointLight {
            pos: glam::vec3(40.0, 40.0, -30.0).into(),
            color: (glam::vec3(1.0, 6.0, 7.0) * 3.0).into(),

            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        renderer.render_context.scene_manager.register_point_light(gpu::PointLight {
            pos: glam::vec3(40.0, 40.0, 30.0).into(),
            color: (glam::vec3(5.0, 1.0, 8.0) * 3.0).into(),

            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        log::info!("Loading scene...");
        AssimpSceneLoader::load_scene(
            TruvisPath::assets_path_str("fbx/cornell-box.fbx").as_ref(),
            &mut renderer.render_context.scene_manager,
            &mut renderer.render_context.asset_hub,
        );
        log::info!("Scene loaded.");
    }
}

impl AppPlugin for CornellApp {
    fn init(&mut self, renderer: &mut Renderer, camera: &mut Camera) {
        let rt_pipeline = RtPipeline::new(
            &renderer.render_context.global_descriptor_sets,
            renderer.render_present.as_ref().unwrap().swapchain.as_ref().unwrap(),
            &mut renderer.cmd_allocator,
        );

        Self::create_scene(renderer, camera);

        self.rt_pipeline = Some(rt_pipeline);
    }

    fn build_ui(&mut self, _ui: &imgui::Ui) {}

    fn update(&mut self, _renderer: &mut Renderer) {}

    fn render(&self, renderer: &Renderer, gui_draw_data: &imgui::DrawData, fence: &GfxSemaphore) {
        self.rt_pipeline.as_ref().unwrap().render(
            &renderer.render_context,
            renderer.render_present.as_ref().unwrap(),
            gui_draw_data,
            fence,
        );
    }
}
