use crate::outer_app::base::OuterApp;
use crate::render_pipeline::rt_render_graph::RtPipeline;
use imgui::Ui;
use truvis_crate_tools::resource::TruvisPath;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_renderer::model_loader::gltf_loader::GltfSceneLoader;
use truvis_renderer::platform::camera::Camera;
use truvis_renderer::renderer::Renderer;
use truvis_shader_binding::truvisl;

/// GLTF 场景演示应用
///
/// 使用 GLTF 加载器加载 `assets/gltf/` 目录下的 `.glb`/`.gltf` 模型，
/// 并通过 RT 渲染管线进行渲染。
#[derive(Default)]
pub struct GltfApp {
    rt_pipeline: Option<RtPipeline>,
}

impl GltfApp {
    fn create_scene(renderer: &mut Renderer, camera: &mut Camera) {
        camera.position = glam::vec3(0.0, 1.0, 3.0);
        camera.euler_yaw_deg = 180.0;
        camera.euler_pitch_deg = 0.0;

        renderer.render_context.scene_manager.register_point_light(truvisl::PointLight {
            pos: glam::vec3(0.0, 3.0, 0.0).into(),
            color: (glam::vec3(5.0, 5.0, 5.0) * 3.0).into(),

            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });

        let model_path = TruvisPath::assets_path("gltf/scene.glb");
        log::info!("start loading gltf scene: {:?}", model_path);
        GltfSceneLoader::load_scene(
            &model_path,
            &mut renderer.render_context.scene_manager,
            &mut renderer.render_context.asset_hub,
        );
        log::info!("finished loading gltf scene");
    }
}

impl OuterApp for GltfApp {
    fn init(&mut self, renderer: &mut Renderer, camera: &mut Camera) {
        let rt_pipeline = RtPipeline::new(
            &renderer.render_context.global_descriptor_sets,
            renderer.render_present.as_ref().unwrap().swapchain.as_ref().unwrap(),
            &mut renderer.cmd_allocator,
        );

        Self::create_scene(renderer, camera);

        self.rt_pipeline = Some(rt_pipeline);
    }

    fn draw_ui(&mut self, _ui: &Ui) {}
    fn update(&mut self, _renderer: &mut Renderer) {}

    fn draw(&self, renderer: &Renderer, gui_draw_data: &imgui::DrawData, fence: &GfxSemaphore) {
        self.rt_pipeline.as_ref().unwrap().render(
            &renderer.render_context,
            renderer.render_present.as_ref().unwrap(),
            gui_draw_data,
            fence,
        );
    }
}
