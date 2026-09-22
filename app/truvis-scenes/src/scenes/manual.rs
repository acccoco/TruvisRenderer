use truvis_asset::handle::TextureColorSpace;
use truvis_path::TruvisPath;
use truvis_shader_binding::gpu;
use truvis_world::GameWorld;
use truvis_world::components::instance::Instance;
use truvis_world::components::material::{CoverageMode, MaterialData, TextureChannel, TextureSlot};
use truvis_world::guid_new_type::{MaterialAssetHandle, MeshAssetHandle};
use truvis_world::procedural_mesh::ProceduralMeshKind;

use renderer_kit::camera::Camera;

use super::MaterialSamples;

pub(super) struct ManualScene;

impl ManualScene {
    const COLUMN_CENTERS: [f32; 5] = [-4.0, -2.0, 0.0, 2.0, 4.0];

    pub(super) fn initialize(world: &mut GameWorld, camera: &mut Camera) {
        camera.position = glam::vec3(10.0, 8.0, 14.0);
        camera.euler_yaw_deg = 38.7;
        camera.euler_pitch_deg = -25.1;

        let samples = MaterialSamples::register(world);
        let cube_mesh =
            world.import_mesh(ProceduralMeshKind::Cube.mesh_data()).expect("failed to register manual cube mesh");
        Self::spawn_ground(world);
        Self::spawn_material_rows(world, &samples, cube_mesh);
        Self::spawn_texture_row(world, cube_mesh);
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

    fn spawn_material_rows(world: &mut GameWorld, samples: &MaterialSamples, cube_mesh: MeshAssetHandle) {
        let sphere_mesh = world
            .import_mesh(ProceduralMeshKind::UvSphere.mesh_data())
            .expect("failed to register manual UV sphere mesh");

        for (index, x) in Self::COLUMN_CENTERS.into_iter().enumerate() {
            let material = samples.get(index);
            Self::spawn_sample(
                world,
                cube_mesh,
                material,
                format!("cube-{}", samples.name(index)),
                glam::vec3(x, 0.5, 1.5),
            );
            Self::spawn_sample(
                world,
                sphere_mesh,
                material,
                format!("sphere-{}", samples.name(index)),
                glam::vec3(x, 0.5, -1.5),
            );
        }
    }

    /// 只注册 CPU 资源身份；解码、GPU 上传与销毁继续由现有资源链路负责。
    fn spawn_texture_row(world: &mut GameWorld, cube_mesh: MeshAssetHandle) {
        let specs = [
            ("uv-checker", "resources/uv_checker.png", CoverageMode::Opaque),
            ("ask", "textures/Ask.jpg", CoverageMode::Opaque),
            ("ri", "textures/Ri.jpg", CoverageMode::Opaque),
            ("babala-opacity", "textures/babala-opacity.png", CoverageMode::Opaque),
            ("babala-transparent", "textures/babala-transparent.png", CoverageMode::alpha_mask(0.5)),
        ];

        for (x, (name, path, coverage)) in Self::COLUMN_CENTERS.into_iter().zip(specs) {
            let texture = world
                .import_texture(TruvisPath::assets(path), TextureColorSpace::Srgb)
                .unwrap_or_else(|error| panic!("failed to import manual texture '{path}': {error}"));
            let mut data = MaterialData {
                name: format!("manual-texture-{name}"),
                roughness: 0.7,
                coverage,
                ..Default::default()
            };
            // 每个面使用完整 UV0；透明样本直接以 BaseColor alpha 裁切，不增加独立遮罩。
            data.textures[TextureChannel::BaseColor as usize] = Some(TextureSlot::new(texture));
            let material = world
                .register_material(data)
                .unwrap_or_else(|error| panic!("failed to register manual texture material '{name}': {error}"));
            Self::spawn_sample(world, cube_mesh, material, format!("cube-texture-{name}"), glam::vec3(x, 0.5, 4.5));
        }
    }

    fn spawn_sample(
        world: &mut GameWorld,
        mesh: MeshAssetHandle,
        material: MaterialAssetHandle,
        name: String,
        position: glam::Vec3,
    ) {
        world
            .create_mesh_instance(Instance {
                name: name.clone(),
                mesh,
                materials: vec![material],
                transform: glam::Mat4::from_translation(position),
            })
            .unwrap_or_else(|error| panic!("failed to register manual sample '{name}': {error}"));
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
