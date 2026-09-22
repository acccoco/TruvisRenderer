use truvis_asset::handle::TextureColorSpace;
use truvis_path::TruvisPath;
use truvis_shader_binding::gpu;
use truvis_world::GameWorld;
use truvis_world::components::instance::Instance;
use truvis_world::components::material::{CoverageMode, MaterialClass, MaterialData, TextureChannel, TextureSlot};
use truvis_world::guid_new_type::{MaterialAssetHandle, MeshAssetHandle};
use truvis_world::procedural_mesh::ProceduralMeshKind;

use renderer_kit::camera::Camera;

use super::MaterialSamples;

pub(super) struct ManualScene;

impl ManualScene {
    const COLUMN_CENTERS: [f32; 5] = [-4.0, -2.0, 0.0, 2.0, 4.0];

    pub(super) fn initialize(world: &mut GameWorld, camera: &mut Camera) {
        camera.position = glam::vec3(11.0, 6.0, 22.0);
        let direction = (glam::vec3(5.5, 1.5, 1.0) - camera.position).normalize();
        camera.euler_yaw_deg = (-direction.x).atan2(-direction.z).to_degrees();
        camera.euler_pitch_deg = direction.y.asin().to_degrees();

        let samples = MaterialSamples::register(world);
        let cube_mesh =
            world.import_mesh(ProceduralMeshKind::Cube.mesh_data()).expect("failed to register manual cube mesh");
        let floor_mesh =
            world.import_mesh(ProceduralMeshKind::Floor.mesh_data()).expect("failed to register manual floor mesh");
        Self::spawn_ground(world, floor_mesh);
        Self::spawn_material_rows(world, &samples, cube_mesh);
        Self::spawn_texture_row(world, cube_mesh);
        Self::spawn_cornell_box(world, cube_mesh, floor_mesh);
        Self::register_lighting(world);
    }

    fn spawn_ground(world: &mut GameWorld, floor_mesh: MeshAssetHandle) {
        let ground_material = MaterialSamples::register_ground(world);

        Self::spawn_instance(
            world,
            floor_mesh,
            ground_material,
            "ground".to_string(),
            glam::Mat4::from_scale(glam::vec3(10.0, 1.0, 8.0)),
        );
    }

    fn spawn_material_rows(world: &mut GameWorld, samples: &MaterialSamples, cube_mesh: MeshAssetHandle) {
        let sphere_mesh = world
            .import_mesh(ProceduralMeshKind::UvSphere.mesh_data())
            .expect("failed to register manual UV sphere mesh");

        for (index, x) in Self::COLUMN_CENTERS.into_iter().enumerate() {
            let material = samples.get(index);
            Self::spawn_instance(
                world,
                cube_mesh,
                material,
                format!("cube-{}", samples.name(index)),
                glam::Mat4::from_translation(glam::vec3(x, 0.5, 1.5)),
            );
            Self::spawn_instance(
                world,
                sphere_mesh,
                material,
                format!("sphere-{}", samples.name(index)),
                glam::Mat4::from_translation(glam::vec3(x, 0.5, -1.5)),
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
            Self::spawn_instance(
                world,
                cube_mesh,
                material,
                format!("cube-texture-{name}"),
                glam::Mat4::from_translation(glam::vec3(x, 0.5, 4.5)),
            );
        }
    }

    /// 箱内地面中心为局部原点，开口朝 +Z；所有实例共享同一世界平移。
    /// 只创建 CPU 材质和实例，mesh 复用 Manual 已注册的资源，生命周期仍归 GameWorld / Runtime。
    fn spawn_cornell_box(world: &mut GameWorld, cube_mesh: MeshAssetHandle, floor_mesh: MeshAssetHandle) {
        let specs = [
            ("white", glam::vec3(0.73, 0.73, 0.73), MaterialClass::Surface),
            ("red", glam::vec3(0.65, 0.05, 0.05), MaterialClass::Surface),
            ("green", glam::vec3(0.05, 0.45, 0.10), MaterialClass::Surface),
            ("light", glam::Vec3::ONE, MaterialClass::emissive(glam::Vec3::splat(15.0))),
        ];
        let [white, red, green, light] = specs.map(|(name, color, class)| {
            world
                .register_material(MaterialData {
                    name: format!("cornell-{name}"),
                    base_color: color.extend(1.0),
                    class,
                    ..Default::default()
                })
                .unwrap_or_else(|error| panic!("failed to register Cornell material '{name}': {error}"))
        });

        // 箱体最左侧为 X=11.4，与原地面右边界 X=10 留出 1.4 m 间隔；全部部件随原点整体平移。
        let origin = glam::vec3(13.5, 0.12, 0.0);
        let blocks = [
            ("floor", white, glam::vec3(0.0, -0.05, -0.05), glam::vec3(4.2, 0.1, 4.1), 0.0_f32),
            ("ceiling", white, glam::vec3(0.0, 4.05, -0.05), glam::vec3(4.2, 0.1, 4.1), 0.0),
            ("left-wall", red, glam::vec3(-2.05, 2.0, -0.05), glam::vec3(0.1, 4.0, 4.1), 0.0),
            ("right-wall", green, glam::vec3(2.05, 2.0, -0.05), glam::vec3(0.1, 4.0, 4.1), 0.0),
            ("back-wall", white, glam::vec3(0.0, 2.0, -2.05), glam::vec3(4.0, 4.0, 0.1), 0.0),
            ("short-block", white, glam::vec3(-0.85, 0.6, 0.7), glam::vec3(1.1, 1.2, 1.1), -15.0),
            ("tall-block", white, glam::vec3(0.75, 1.2, -0.65), glam::vec3(1.1, 2.4, 1.1), 18.0),
        ];
        for (name, material, center, size, yaw_deg) in blocks {
            Self::spawn_instance(
                world,
                cube_mesh,
                material,
                format!("cornell-{name}"),
                glam::Mat4::from_scale_rotation_translation(
                    size,
                    glam::Quat::from_rotation_y(yaw_deg.to_radians()),
                    origin + center,
                ),
            );
        }

        // Floor 边长为 2，翻转朝下并距顶板 0.02 m；自发光已参与 NEE，不叠加 analytic light 的能量。
        Self::spawn_instance(
            world,
            floor_mesh,
            light,
            "cornell-light".to_string(),
            glam::Mat4::from_scale_rotation_translation(
                glam::vec3(0.6, 1.0, 0.5),
                glam::Quat::from_rotation_x(std::f32::consts::PI),
                origin + glam::vec3(0.0, 3.98, -0.2),
            ),
        );
    }

    fn spawn_instance(
        world: &mut GameWorld,
        mesh: MeshAssetHandle,
        material: MaterialAssetHandle,
        name: String,
        transform: glam::Mat4,
    ) {
        world
            .create_mesh_instance(Instance {
                name: name.clone(),
                mesh,
                materials: vec![material],
                transform,
            })
            .unwrap_or_else(|error| panic!("failed to register manual instance '{name}': {error}"));
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
