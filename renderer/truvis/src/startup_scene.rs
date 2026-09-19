use std::path::PathBuf;

use truvis_asset::handle::LoadStatus;
use truvis_asset::scene_manifest::SceneManifest;
use truvis_world::World;

use renderer_kit::camera::Camera;

use crate::bistro_scene::BistroScene;

/// Renderer 选择启动场景的最小 controller。
///
/// 场景几何、相机和灯光表达在 manifest/Asset 层；这里仅把 CPU 场景请求接到
/// `World`，并把相机 preset 投影到 Renderer 自己拥有的 `Camera`。
pub(crate) enum StartupScene {
    Manifest(ManifestScene),
    Bistro(BistroScene),
}

pub(crate) struct ManifestScene {
    manifest: SceneManifest,
    import: Option<truvis_world::guid_new_type::ModelImportHandle>,
}

impl Default for StartupScene {
    fn default() -> Self {
        match std::env::var_os("TRUVIS_SCENE_MANIFEST") {
            Some(path) => {
                let path = PathBuf::from(path);
                let manifest = SceneManifest::load(&path)
                    .unwrap_or_else(|error| panic!("failed to load scene manifest {}: {error}", path.display()));
                Self::Manifest(ManifestScene { manifest, import: None })
            }
            None => Self::Bistro(BistroScene::default()),
        }
    }
}

impl StartupScene {
    pub(crate) fn request(&mut self, world: &mut World, camera: &mut Camera) {
        match self {
            Self::Bistro(scene) => scene.request(world, camera),
            Self::Manifest(scene) => scene.request(world, camera),
        }
    }

    pub(crate) fn update(&mut self, world: &World) {
        match self {
            Self::Bistro(scene) => scene.update(world),
            Self::Manifest(scene) => scene.update(world),
        }
    }
}

impl ManifestScene {
    fn request(&mut self, world: &mut World, camera: &mut Camera) {
        let preset = self
            .manifest
            .cameras
            .iter()
            .find(|camera| camera.name == self.manifest.default_camera)
            .unwrap_or_else(|| panic!("scene manifest '{}' has no default camera '{}'", self.manifest.name, self.manifest.default_camera));
        camera.position = glam::Vec3::from_array(preset.position);
        let (yaw, pitch, roll) =
            glam::Quat::from_xyzw(preset.rotation[0], preset.rotation[1], preset.rotation[2], preset.rotation[3])
                .to_euler(glam::EulerRot::YXZ);
        camera.euler_yaw_deg = yaw.to_degrees();
        camera.euler_pitch_deg = pitch.to_degrees();
        camera.euler_roll_deg = roll.to_degrees();
        camera.fov_deg_vertical = preset.fov_deg;
        camera.near = preset.near;

        if let Some(sky) = &self.manifest.sky {
            world
                .request_sky_texture_from_path(sky.clone())
                .unwrap_or_else(|error| panic!("scene manifest '{}' sky request failed: {error}", self.manifest.name));
        }
        world.register_area_lights(&self.manifest.area_lights);
        self.import = Some(world.request_model_import(self.manifest.model.clone()));
        log::info!(
            "Scene requested: {} (model={}, cameras={}, area_lights={})",
            self.manifest.name,
            self.manifest.model.display(),
            self.manifest.cameras.len(),
            self.manifest.area_lights.len()
        );
    }

    fn update(&mut self, world: &World) {
        let Some(handle) = self.import else { return };
        match world.model_import_status(handle) {
            LoadStatus::Ready => {
                log::info!("Scene CPU import ready: {}; GPU uploads continue in Runtime", self.manifest.name);
                self.import = None;
            }
            LoadStatus::Failed => {
                log::error!("Scene import failed: {}: {:?}", self.manifest.name, world.model_import_error(handle));
                self.import = None;
            }
            _ => {}
        }
    }
}
