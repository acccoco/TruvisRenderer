use truvis_shader_binding::gpu;
use truvis_world::GameWorld;
use truvis_world::components::instance::Instance;
use truvis_world::procedural_mesh::ProceduralMeshKind;

use renderer_kit::camera::Camera;

use super::MaterialSamples;

pub(super) struct ManualScene;

impl ManualScene {
    pub(super) fn initialize(world: &mut GameWorld, camera: &mut Camera) {
        camera.position = glam::vec3(8.0, 6.0, 10.0);
        camera.euler_yaw_deg = 38.7;
        camera.euler_pitch_deg = -23.2;

        let samples = MaterialSamples::register(world);
        Self::spawn_ground(world);
        Self::spawn_material_rows(world, &samples);
        Self::register_lighting(world);
    }

    fn spawn_ground(world: &mut GameWorld) {
        let ground_mesh =
            world.import_mesh(ProceduralMeshKind::Floor.mesh_data()).expect("failed to register manual ground mesh");
        let ground_material = MaterialSamples::register_ground(world);

        world
            .create_mesh_instance(Instance {
                name: "ground".to_string(),
                mesh: ground_mesh,
                materials: vec![ground_material],
                transform: glam::Mat4::from_scale(glam::vec3(10.0, 1.0, 8.0)),
            })
            .expect("failed to register manual ground instance");
    }

    fn spawn_material_rows(world: &mut GameWorld, samples: &MaterialSamples) {
        let cube_mesh =
            world.import_mesh(ProceduralMeshKind::Cube.mesh_data()).expect("failed to register manual cube mesh");
        let sphere_mesh = world
            .import_mesh(ProceduralMeshKind::UvSphere.mesh_data())
            .expect("failed to register manual UV sphere mesh");
        let centers = [-4.0_f32, -2.0, 0.0, 2.0, 4.0];

        for (index, x) in centers.into_iter().enumerate() {
            let material = samples.get(index);
            world
                .create_mesh_instance(Instance {
                    name: format!("cube-{}", samples.name(index)),
                    mesh: cube_mesh,
                    materials: vec![material],
                    transform: glam::Mat4::from_translation(glam::vec3(x, 0.5, 1.5)),
                })
                .expect("failed to register manual cube instance");
            world
                .create_mesh_instance(Instance {
                    name: format!("sphere-{}", samples.name(index)),
                    mesh: sphere_mesh,
                    materials: vec![material],
                    transform: glam::Mat4::from_translation(glam::vec3(x, 0.5, -1.5)),
                })
                .expect("failed to register manual UV sphere instance");
        }
    }

    fn register_lighting(world: &mut GameWorld) {
        world.register_area_light(gpu::engine::light::AreaLight {
            center: glam::vec3(0.0, 6.0, 2.0).into(),
            half_u: glam::vec3(3.0, 0.0, 0.0).into(),
            half_v: glam::vec3(0.0, 0.0, 2.0).into(),
            radiance: glam::vec3(8.0, 8.0, 8.0).into(),
            _center_padding: Default::default(),
            _half_u_padding: Default::default(),
            _half_v_padding: Default::default(),
            _radiance_padding: Default::default(),
        });

        world.register_point_light(gpu::engine::light::PointLight {
            pos: glam::vec3(-4.0, 3.0, 2.0).into(),
            color: (glam::vec3(1.0, 0.45, 0.25) * 150.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        world.register_point_light(gpu::engine::light::PointLight {
            pos: glam::vec3(4.0, 3.0, 2.0).into(),
            color: (glam::vec3(0.25, 0.45, 1.0) * 150.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
    }
}
