use renderer_kit::camera::Camera;
use truvis_path::TruvisPath;
use truvis_shader_binding::gpu;
use truvis_world::GameWorld;
use truvis_world::guid_new_type::SceneImportHandle;

pub(super) struct CornellScene;

impl CornellScene {
    pub(super) const IMPORT_PATH: &'static str = "fbx/cornell-box.fbx";

    pub(super) fn initialize(world: &mut GameWorld, camera: &mut Camera) -> SceneImportHandle {
        camera.position = glam::vec3(-4.0, 10.0, 10.0);
        camera.euler_yaw_deg = 330.0;
        camera.euler_pitch_deg = -27.0;

        world.register_point_light(gpu::engine::light::PointLight {
            pos: glam::vec3(-0.2, 0.4, 0.0).into(),
            color: (glam::vec3(5.0, 6.0, 1.0) * 2.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        world.register_point_light(gpu::engine::light::PointLight {
            pos: glam::vec3(0.4, 0.4, -0.3).into(),
            color: (glam::vec3(1.0, 6.0, 7.0) * 3.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        world.register_point_light(gpu::engine::light::PointLight {
            pos: glam::vec3(0.4, 0.4, 0.3).into(),
            color: (glam::vec3(5.0, 1.0, 8.0) * 3.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        world.register_spot_light(gpu::engine::light::SpotLight {
            pos: glam::vec3(0.0, 3.2, 1.8).into(),
            inner_angle: 12.0_f32.to_radians(),
            color: (glam::vec3(8.0, 6.0, 3.0) * 8.0).into(),
            outer_angle: 28.0_f32.to_radians(),
            dir: glam::vec3(0.0, -0.85, -0.35).normalize().into(),
            _dir_padding: Default::default(),
        });
        world.register_area_light(gpu::engine::light::AreaLight {
            center: glam::vec3(0.0, 3.8, 0.0).into(),
            half_u: glam::vec3(0.8, 0.0, 0.0).into(),
            half_v: glam::vec3(0.0, 0.0, 0.8).into(),
            radiance: (glam::vec3(1.0, 0.92, 0.75) * 2.0).into(),
            _center_padding: Default::default(),
            _half_u_padding: Default::default(),
            _half_v_padding: Default::default(),
            _radiance_padding: Default::default(),
        });

        log::info!("Loading model...");
        world.import_scene(TruvisPath::assets_path(Self::IMPORT_PATH))
    }
}
