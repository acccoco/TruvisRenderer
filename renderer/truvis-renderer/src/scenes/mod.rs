mod manual;
mod sponza;

use truvis_asset::handle::LoadStatus;
use truvis_world::GameWorld;
use truvis_world::components::instance::Instance;
use truvis_world::components::material::{CoverageMode, MaterialClass, MaterialData};
use truvis_world::guid_new_type::{MaterialAssetHandle, SceneImportHandle};

use renderer_kit::camera::Camera;

/// Renderer 启动时选择的固定场景预设。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InitialScene {
    #[default]
    Manual,
    Sponza,
}

impl InitialScene {
    pub fn parse_name(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "sponza" => Some(Self::Sponza),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Sponza => "sponza",
        }
    }
}

/// 场景初始化和异步导入状态的唯一 owner。
///
/// `GameWorld` 仍然拥有最终 CPU scene；本类型只保存启动预设和一个待收敛的
/// import handle，不保存 GPU 资源、Runtime 或跨线程状态。
pub(crate) struct SceneInitializer {
    preset: InitialScene,
    pending_scene_import: Option<SceneImportHandle>,
}

impl SceneInitializer {
    pub(crate) fn new(preset: InitialScene) -> Self {
        Self {
            preset,
            pending_scene_import: None,
        }
    }

    pub(crate) fn initialize(&mut self, world: &mut GameWorld, camera: &mut Camera) {
        self.pending_scene_import = match self.preset {
            InitialScene::Manual => {
                manual::ManualScene::initialize(world, camera);
                None
            }
            InitialScene::Sponza => Some(sponza::SponzaScene::initialize(world, camera)),
        };
    }

    pub(crate) fn update(&mut self, world: &mut GameWorld) {
        let Some(handle) = self.pending_scene_import else {
            return;
        };

        match world.scene_import_status(handle) {
            LoadStatus::Unloaded | LoadStatus::Loading => {}
            LoadStatus::Failed => {
                let error = world.scene_import_error(handle).unwrap_or("unknown scene import error");
                log::error!("failed to load {} scene: {error}", self.preset.name());
                self.pending_scene_import = None;
            }
            LoadStatus::Ready => {
                let Some(scene_data) = world.scene_data(handle).cloned() else {
                    log::error!("{} scene import became ready without scene data", self.preset.name());
                    self.pending_scene_import = None;
                    return;
                };

                for object in scene_data.objects {
                    if let Err(error) = world.create_mesh_instance(Instance {
                        name: object.name,
                        mesh: object.mesh,
                        materials: object.materials,
                        transform: object.transform,
                    }) {
                        log::error!("failed to register {} scene instance: {error}", self.preset.name());
                        break;
                    }
                }
                self.pending_scene_import = None;
            }
        }
    }
}

/// 两个启动场景共用的五种材质样本。
///
/// 只保存 CPU material handles；每个场景仍独立决定这些材质如何绑定到几何实例。
pub(super) struct MaterialSamples {
    handles: [MaterialAssetHandle; 5],
}

impl MaterialSamples {
    const NAMES: [&'static str; 5] = [
        "glass",
        "mirror",
        "glossy-plastic",
        "rough-plastic",
        "emissive-reference",
    ];

    pub(super) fn register(world: &mut GameWorld) -> Self {
        let specs = [
            (
                Self::NAMES[0],
                glam::vec4(0.65, 0.85, 1.0, 1.0),
                0.0,
                0.0,
                MaterialClass::transmission(0.25, MaterialClass::DEFAULT_IOR),
            ),
            (Self::NAMES[1], glam::vec4(0.96, 0.96, 0.92, 1.0), 1.0, 0.0, MaterialClass::Surface),
            (Self::NAMES[2], glam::vec4(0.95, 0.08, 0.18, 1.0), 0.0, 0.18, MaterialClass::Surface),
            (Self::NAMES[3], glam::vec4(0.18, 0.95, 0.25, 1.0), 0.0, 0.75, MaterialClass::Surface),
            (
                Self::NAMES[4],
                glam::vec4(1.0, 0.65, 0.18, 1.0),
                0.0,
                1.0,
                MaterialClass::emissive(glam::vec3(4.0, 2.2, 0.5)),
            ),
        ];

        let handles = specs.map(|(name, base_color, metallic, roughness, class)| {
            world
                .register_material(MaterialData {
                    base_color,
                    metallic,
                    roughness,
                    class,
                    coverage: CoverageMode::Opaque,
                    textures: Default::default(),
                    normal_scale: 1.0,
                    emissive_factor: glam::Vec3::ZERO,
                    name: format!("material-test-{name}"),
                })
                .expect("failed to register material sample")
        });

        Self { handles }
    }

    pub(super) fn get(&self, index: usize) -> MaterialAssetHandle {
        self.handles[index]
    }

    pub(super) fn name(&self, index: usize) -> &'static str {
        Self::NAMES[index]
    }

    pub(super) fn register_ground(world: &mut GameWorld) -> MaterialAssetHandle {
        world
            .register_material(MaterialData {
                base_color: glam::vec4(0.32, 0.34, 0.38, 1.0),
                metallic: 0.0,
                roughness: 0.85,
                class: MaterialClass::Surface,
                coverage: CoverageMode::Opaque,
                textures: Default::default(),
                normal_scale: 1.0,
                emissive_factor: glam::Vec3::ZERO,
                name: "manual-ground".to_string(),
            })
            .expect("failed to register manual ground material")
    }
}
