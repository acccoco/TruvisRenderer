use serde::Deserialize;

use renderer_kit::camera::Camera;
use truvis_asset::handle::LoadStatus;
use truvis_path::TruvisPath;
use truvis_world::{World, guid_new_type::ModelImportHandle};

/// 产品默认场景及其一次性 CPU 导入状态；资源加载和 GPU ready 仍由 World/Runtime 推进。
pub(crate) struct BistroScene {
    name: &'static str,
    import: Option<ModelImportHandle>,
}

#[derive(Deserialize)]
struct BistroCamera {
    position: [f32; 3],
    yaw_deg: f32,
    pitch_deg: f32,
    fov_deg: f32,
}

impl Default for BistroScene {
    fn default() -> Self {
        let requested = std::env::var("TRUVIS_BISTRO_SCENE").unwrap_or_else(|_| "interior".into());
        let name = match requested.as_str() {
            "exterior" => "BistroExterior",
            "interior" => "BistroInterior",
            "interior-wine" => "BistroInterior_Wine",
            _ => panic!("TRUVIS_BISTRO_SCENE must be exterior, interior, or interior-wine; got {requested}"),
        };
        Self { name, import: None }
    }
}

impl BistroScene {
    pub(crate) fn request(&mut self, world: &mut World, camera: &mut Camera) {
        let root = TruvisPath::assets_path("gltf/bistro");
        let path = root.join(format!("{}.gltf", self.name));
        assert!(
            path.is_file(),
            "Bistro asset missing: {}. Run just prepare-bistro <Bistro_v5_2 path> first",
            path.display()
        );
        let view: BistroCamera = serde_json::from_slice(
            &std::fs::read(root.join(format!("{}.camera.json", self.name))).expect("Bistro camera metadata missing"),
        )
        .expect("invalid Bistro camera metadata");
        camera.position = glam::Vec3::from_array(view.position);
        camera.euler_yaw_deg = view.yaw_deg;
        camera.euler_pitch_deg = view.pitch_deg;
        camera.fov_deg_vertical = view.fov_deg;
        camera.near = 0.05;
        world
            .request_sky_texture_from_path(root.join("san_giuseppe_bridge_4k.hdr"))
            .expect("failed to request Bistro HDRI");
        self.import = Some(world.request_model_import(path));
        log::info!("Bistro scene requested: {}", self.name);
    }

    pub(crate) fn update(&mut self, world: &World) {
        let Some(handle) = self.import else {
            return;
        };
        match world.model_import_status(handle) {
            LoadStatus::Ready => {
                log::info!("Bistro CPU import ready: {}; GPU uploads continue in Runtime", self.name);
                self.import = None;
            }
            LoadStatus::Failed => {
                log::error!("Bistro import failed: {}: {:?}", self.name, world.model_import_error(handle));
                self.import = None;
            }
            _ => {}
        }
    }
}
