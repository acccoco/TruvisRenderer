use truvis_path::TruvisPath;
use truvis_shader_binding::gpu;
use truvis_world::GameWorld;
use truvis_world::components::instance::Instance;
use truvis_world::components::material::{CoverageMode, MaterialClass, MaterialData};
use truvis_world::guid_new_type::{MeshAssetHandle, SceneImportHandle};
use truvis_world::procedural_mesh::ProceduralMeshKind;

use renderer_kit::camera::Camera;

use super::MaterialSamples;

pub(super) struct SponzaScene;

#[derive(Clone, Copy)]
struct EmissiveCubeMatrixConfig {
    start_offset: glam::Vec3,
    spacing: glam::Vec3,
    cube_scale: f32,
    counts: glam::UVec3,
}

const EMISSIVE_CUBE_MATRIX_CONFIG: EmissiveCubeMatrixConfig = EmissiveCubeMatrixConfig {
    start_offset: glam::Vec3::new(-8.0, 6.0, -4.25),
    spacing: glam::Vec3::new(0.75, 0.6, 0.9),
    cube_scale: 0.1,
    counts: glam::UVec3::new(20, 1, 10),
};

impl SponzaScene {
    pub(super) fn initialize(world: &mut GameWorld, camera: &mut Camera) -> SceneImportHandle {
        camera.position = glam::vec3(2.7, 1.94, -0.64);
        camera.euler_yaw_deg = 90.0;
        camera.euler_pitch_deg = 0.0;

        Self::register_lighting(world);
        let samples = MaterialSamples::register(world);
        Self::spawn_material_test_cubes(world, &samples);

        log::info!("start load sponza model");
        world.import_scene(TruvisPath::assets_path("fbx/sponza/sponza.fbx"))
    }

    fn spawn_material_test_cubes(world: &mut GameWorld, samples: &MaterialSamples) {
        let cube_mesh =
            world.import_mesh(ProceduralMeshKind::Cube.mesh_data()).expect("failed to register procedural cube mesh");
        let centers = [
            glam::vec3(-8.0, 1.0, -0.25),
            glam::vec3(-4.5, 1.0, -0.25),
            glam::vec3(-1.0, 1.0, -0.25),
            glam::vec3(2.5, 1.0, -0.25),
            glam::vec3(6.0, 1.0, -0.25),
        ];

        for (index, center) in centers.into_iter().enumerate() {
            world
                .create_mesh_instance(Instance {
                    name: format!("material-test-cube-{}", samples.name(index)),
                    mesh: cube_mesh,
                    materials: vec![samples.get(index)],
                    transform: glam::Mat4::from_translation(center),
                })
                .expect("failed to register material test cube instance");
        }

        Self::spawn_emissive_cube_matrix(world, cube_mesh);
    }

    fn spawn_emissive_cube_matrix(world: &mut GameWorld, cube_mesh: MeshAssetHandle) {
        let palette_specs = [
            ("warm-amber", glam::vec4(1.0, 0.72, 0.32, 1.0), glam::vec3(4.8, 2.7, 0.8) * 5.0),
            ("rose", glam::vec4(1.0, 0.36, 0.54, 1.0), glam::vec3(4.2, 0.9, 1.8) * 5.0),
            ("cyan", glam::vec4(0.42, 0.95, 1.0, 1.0), glam::vec3(1.2, 3.8, 4.8) * 5.0),
            ("lime", glam::vec4(0.54, 1.0, 0.38, 1.0), glam::vec3(1.4, 4.5, 1.0) * 5.0),
            ("violet", glam::vec4(0.72, 0.48, 1.0, 1.0), glam::vec3(2.2, 1.2, 4.8) * 5.0),
        ];
        let emissive_materials = palette_specs.map(|(name, base_color, radiance)| {
            world
                .register_material(MaterialData {
                    base_color,
                    metallic: 0.0,
                    roughness: 1.0,
                    class: MaterialClass::emissive(radiance),
                    coverage: CoverageMode::Opaque,
                    textures: Default::default(),
                    normal_scale: 1.0,
                    emissive_factor: glam::Vec3::ZERO,
                    name: format!("emissive-cube-matrix-{name}"),
                })
                .expect("failed to register emissive cube material")
        });

        let mut cube_index = 0usize;
        for y in 0..EMISSIVE_CUBE_MATRIX_CONFIG.counts.y {
            for z in 0..EMISSIVE_CUBE_MATRIX_CONFIG.counts.z {
                for x in 0..EMISSIVE_CUBE_MATRIX_CONFIG.counts.x {
                    let center = EMISSIVE_CUBE_MATRIX_CONFIG.start_offset
                        + glam::vec3(
                            x as f32 * EMISSIVE_CUBE_MATRIX_CONFIG.spacing.x,
                            y as f32 * EMISSIVE_CUBE_MATRIX_CONFIG.spacing.y,
                            z as f32 * EMISSIVE_CUBE_MATRIX_CONFIG.spacing.z,
                        );
                    world
                        .create_mesh_instance(Instance {
                            name: format!("emissive-cube-matrix-{x}-{y}-{z}"),
                            mesh: cube_mesh,
                            materials: vec![emissive_materials[cube_index % emissive_materials.len()]],
                            transform: glam::Mat4::from_scale_rotation_translation(
                                glam::Vec3::splat(EMISSIVE_CUBE_MATRIX_CONFIG.cube_scale),
                                glam::Quat::IDENTITY,
                                center,
                            ),
                        })
                        .expect("failed to register emissive cube instance");
                    cube_index += 1;
                }
            }
        }
    }

    fn register_lighting(world: &mut GameWorld) {
        world.register_point_light(gpu::engine::light::PointLight {
            pos: glam::vec3(-8.0, 0.5, 4.0).into(),
            color: (glam::vec3(1.0, 0.0, 0.0) * 5000.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        world.register_point_light(gpu::engine::light::PointLight {
            pos: glam::vec3(-1.0, 0.5, 4.0).into(),
            color: (glam::vec3(0.0, 1.0, 0.0) * 5000.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        world.register_point_light(gpu::engine::light::PointLight {
            pos: glam::vec3(6.0, 0.5, 4.0).into(),
            color: (glam::vec3(0.0, 0.0, 1.0) * 5000.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        world.register_spot_light(gpu::engine::light::SpotLight {
            pos: glam::vec3(-4.5, 1.0, 4.0).into(),
            inner_angle: 30.0_f32.to_radians(),
            color: (glam::vec3(1.0, 1.0, 0.0) * 9000.0).into(),
            outer_angle: 60.0_f32.to_radians(),
            dir: glam::vec3(0.0, -1.0, 0.0).into(),
            _dir_padding: Default::default(),
        });
        world.register_spot_light(gpu::engine::light::SpotLight {
            pos: glam::vec3(2.5, 1.0, 4.0).into(),
            inner_angle: 30.0_f32.to_radians(),
            color: (glam::vec3(0.0, 1.0, 1.0) * 9000.0).into(),
            outer_angle: 60.0_f32.to_radians(),
            dir: glam::vec3(0.0, -1.0, 0.0).into(),
            _dir_padding: Default::default(),
        });
        world.register_area_light(gpu::engine::light::AreaLight {
            center: glam::vec3(-1.0, 2.0, 4.0).into(),
            half_u: glam::vec3(0.7, 0.0, 0.0).into(),
            half_v: glam::vec3(0.0, 0.0, 0.18).into(),
            radiance: (glam::vec3(1.0, 0.16, 0.12) * 10.0).into(),
            _center_padding: Default::default(),
            _half_u_padding: Default::default(),
            _half_v_padding: Default::default(),
            _radiance_padding: Default::default(),
        });
        world.register_area_light(gpu::engine::light::AreaLight {
            center: glam::vec3(6.0, 2.0, 4.0).into(),
            half_u: glam::vec3(0.26, 0.0, 0.0).into(),
            half_v: glam::vec3(0.0, 0.0, 0.26).into(),
            radiance: (glam::vec3(0.12, 0.16, 1.0) * 10.0).into(),
            _center_padding: Default::default(),
            _half_u_padding: Default::default(),
            _half_v_padding: Default::default(),
            _radiance_padding: Default::default(),
        });
    }
}
