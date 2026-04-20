use crate::app_plugin::{AppPlugin, InitCtx, RenderCtx, UpdateCtx};
use crate::render_pipeline::rt_render_graph::RtPipeline;
use truvis_asset::asset_hub::AssetHub;
use truvis_path::TruvisPath;
use truvis_renderer::model_loader::assimp_loader::AssimpSceneLoader;
use truvis_renderer::platform::camera::Camera;
use truvis_scene::scene_manager::SceneManager;
use truvis_shader_binding::gpu;

#[derive(Default)]
pub struct SponzaApp {
    rt_pipeline: Option<RtPipeline>,
}

impl SponzaApp {
    fn create_scene(scene_manager: &mut SceneManager, asset_hub: &mut AssetHub, camera: &mut Camera) {
        camera.position = glam::vec3(270.0, 194.0, -64.0);
        camera.euler_yaw_deg = 90.0;
        camera.euler_pitch_deg = 0.0;

        scene_manager.register_point_light(gpu::PointLight {
            pos: glam::vec3(-20.0, 40.0, 0.0).into(),
            color: (glam::vec3(5.0, 6.0, 1.0) * 2.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        scene_manager.register_point_light(gpu::PointLight {
            pos: glam::vec3(40.0, 40.0, -30.0).into(),
            color: (glam::vec3(1.0, 6.0, 7.0) * 3.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        scene_manager.register_point_light(gpu::PointLight {
            pos: glam::vec3(40.0, 40.0, 30.0).into(),
            color: (glam::vec3(5.0, 1.0, 8.0) * 3.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        log::info!("start load sponza scene");
        AssimpSceneLoader::load_scene(
            &TruvisPath::assets_path("fbx/sponza/sponza.fbx"),
            scene_manager,
            asset_hub,
        );
        log::info!("finished load sponza scene");
    }
}

impl AppPlugin for SponzaApp {
    fn init(&mut self, ctx: &mut InitCtx) {
        let rt_pipeline = RtPipeline::new(
            ctx.global_descriptor_sets,
            ctx.render_present.swapchain.as_ref().unwrap(),
            ctx.cmd_allocator,
        );

        Self::create_scene(ctx.scene_manager, ctx.asset_hub, ctx.camera);

        self.rt_pipeline = Some(rt_pipeline);
    }

    fn build_ui(&mut self, _ui: &imgui::Ui) {}
    fn update(&mut self, _ctx: &mut UpdateCtx) {}

    fn render(&self, ctx: &RenderCtx) {
        self.rt_pipeline.as_ref().unwrap().render(
            ctx.render_context,
            ctx.render_present,
            ctx.gui_draw_data,
            ctx.timeline,
        );
    }
}
