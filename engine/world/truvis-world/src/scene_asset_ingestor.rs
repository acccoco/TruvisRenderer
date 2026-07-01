use std::collections::HashMap;
use std::path::{Path, PathBuf};

use slotmap::{SecondaryMap, SlotMap};
use truvis_asset::asset_hub::{AssetHub, AssetLoadEvent};
use truvis_asset::handle::{
    LoadStatus, MeshData, ModelLoadDesc, ModelLoadHandle, RawMaterialData, RawSceneData, TextureLoadDesc,
    TextureLoadHandle,
};

use crate::components::instance::Instance;
use crate::components::material::MaterialData;
use crate::edit_error::SceneEditError;
use crate::guid_new_type::{InstanceHandle, MaterialHandle, MeshHandle, ModelImportHandle, TextureHandle};
use crate::scene_store::SceneStore;
use crate::{FailedTextureLoad, PendingMeshUpload, PendingTextureUpload, SceneAssetSyncOutput};

/// `World` 内部的 scene asset ingest 协调器。
///
/// 它是 loader handle 和 CPU world resource handle 的唯一翻译边界。`AssetHub` 只交付一次性 CPU
/// payload；本对象负责把 model / texture ingest 到 `SceneStore`，并产出 render-side
/// manager 消费的短期 upload event。
#[derive(Default)]
pub struct SceneAssetIngestor {
    model_imports: SlotMap<ModelImportHandle, SceneModelImportRecord>,
    model_loads: SecondaryMap<ModelLoadHandle, ModelImportHandle>,
    texture_loads: SecondaryMap<TextureLoadHandle, TextureHandle>,
    texture_paths: HashMap<PathBuf, TextureHandle>,
    pending_asset_sync: SceneAssetSyncOutput,
}

struct SceneModelImportRecord {
    status: LoadStatus,
    error: Option<String>,
    spawned_instances: Option<Vec<InstanceHandle>>,
}

impl SceneAssetIngestor {
    /// 创建空的 scene asset ingest 状态。
    pub fn new() -> Self {
        Self::default()
    }

    /// 提交一次 model import 请求。
    pub fn request_model_import(&mut self, assets: &mut AssetHub, path: PathBuf) -> ModelImportHandle {
        let scene_import = self.model_imports.insert(SceneModelImportRecord {
            status: LoadStatus::Loading,
            error: None,
            spawned_instances: None,
        });
        let path = match std::fs::canonicalize(&path) {
            Ok(path) => path,
            Err(err) => {
                self.fail_scene_import(scene_import, format!("failed to canonicalize model path: {err}"));
                return scene_import;
            }
        };

        let model_load = assets.request_model(ModelLoadDesc { path });
        self.model_loads.insert(model_load, scene_import);
        scene_import
    }

    /// 注册一个已经 canonicalize 的 file texture，并在必要时提交一次性 CPU texture load task。
    pub fn register_texture_canonical(
        &mut self,
        assets: &mut AssetHub,
        scene: &mut SceneStore,
        path: PathBuf,
    ) -> TextureHandle {
        if let Some(&scene_texture) = self.texture_paths.get(&path) {
            if scene.contains_texture(scene_texture) {
                return scene_texture;
            }
        }

        let texture_load = assets.request_texture(TextureLoadDesc { path: path.clone() });
        let scene_texture = scene.register_texture();
        self.texture_loads.insert(texture_load, scene_texture);
        self.texture_paths.insert(path, scene_texture);
        scene_texture
    }

    /// 注册 CPU mesh payload，并返回 CPU world mesh handle。
    pub fn register_mesh(&mut self, scene: &mut SceneStore, data: MeshData) -> MeshHandle {
        let scene_mesh = scene.register_mesh();
        self.pending_asset_sync.pending_mesh_uploads.push(PendingMeshUpload {
            handle: scene_mesh,
            data,
        });
        scene_mesh
    }

    /// 注册 CPU material 参数，并返回 CPU world material handle。
    pub fn register_material(
        &mut self,
        scene: &mut SceneStore,
        data: MaterialData,
    ) -> Result<MaterialHandle, SceneEditError> {
        scene.register_material(data)
    }

    /// 查询 model import 的当前 CPU 加载状态。
    pub fn model_import_status(&self, handle: ModelImportHandle) -> LoadStatus {
        let Some(record) = self.model_imports.get(handle) else {
            return LoadStatus::Failed;
        };
        record.status
    }

    /// 查询 model import 的失败文本。
    pub fn model_import_error(&self, handle: ModelImportHandle) -> Option<&str> {
        let record = self.model_imports.get(handle)?;
        record.error.as_deref()
    }

    /// 消费 `AssetHub` 完成事件，并转换为 render-side 只看得见 CPU resource handle 的事件。
    pub fn ingest_asset_events(
        &mut self,
        assets: &mut AssetHub,
        scene: &mut SceneStore,
        events: Vec<AssetLoadEvent>,
    ) -> SceneAssetSyncOutput {
        let mut asset_sync = self.drain_pending_asset_sync(scene);
        for event in events {
            self.ingest_asset_event(assets, scene, event, &mut asset_sync);
        }
        asset_sync
    }

    fn ingest_asset_event(
        &mut self,
        assets: &mut AssetHub,
        scene: &mut SceneStore,
        event: AssetLoadEvent,
        asset_sync: &mut SceneAssetSyncOutput,
    ) {
        match event {
            AssetLoadEvent::TextureLoaded { handle, desc: _, data } => {
                let scene_texture = self.take_scene_texture_for_load(handle);
                if !scene.contains_texture(scene_texture) {
                    return;
                }
                asset_sync.pending_texture_uploads.push(PendingTextureUpload {
                    handle: scene_texture,
                    data,
                });
            }
            AssetLoadEvent::TextureFailed { handle, desc: _, error } => {
                let scene_texture = self.take_scene_texture_for_load(handle);
                if !scene.contains_texture(scene_texture) {
                    return;
                }
                asset_sync.failed_textures.push(FailedTextureLoad {
                    handle: scene_texture,
                    error,
                });
            }
            AssetLoadEvent::ModelLoaded { handle, desc: _, data } => {
                self.ingest_model_loaded(assets, scene, handle, data, asset_sync);
            }
            AssetLoadEvent::ModelFailed { handle, desc: _, error } => {
                self.mark_model_failed(handle, error);
            }
        }
    }

    fn ingest_model_loaded(
        &mut self,
        assets: &mut AssetHub,
        scene: &mut SceneStore,
        model_load: ModelLoadHandle,
        raw: RawSceneData,
        asset_sync: &mut SceneAssetSyncOutput,
    ) {
        let scene_import = self.take_scene_import_for_load(model_load);
        if let Err(error) = Self::validate_model_indices(&raw) {
            self.fail_scene_import(scene_import, error);
            return;
        }

        let source_path = raw.source_path.clone();
        if let Err(error) = Self::validate_model_texture_paths(&source_path, &raw.materials) {
            self.fail_scene_import(scene_import, error);
            return;
        }

        let mut scene_meshes = Vec::with_capacity(raw.meshes.len());
        for mesh_data in raw.meshes {
            let scene_mesh = scene.register_mesh();
            asset_sync.pending_mesh_uploads.push(PendingMeshUpload {
                handle: scene_mesh,
                data: mesh_data,
            });
            scene_meshes.push(scene_mesh);
        }

        let mut scene_materials = Vec::with_capacity(raw.materials.len());
        for material in raw.materials {
            let scene_data = MaterialData {
                base_color: material.base_color,
                emissive: material.emissive,
                metallic: material.metallic,
                roughness: material.roughness,
                opaque: material.opaque,
                diffuse_texture: match self.register_model_texture_ref(
                    assets,
                    scene,
                    &source_path,
                    material.diffuse_texture_path,
                    "diffuse",
                ) {
                    Ok(texture) => texture,
                    Err(err) => {
                        self.fail_scene_import(scene_import, err);
                        return;
                    }
                },
                normal_texture: match self.register_model_texture_ref(
                    assets,
                    scene,
                    &source_path,
                    material.normal_texture_path,
                    "normal",
                ) {
                    Ok(texture) => texture,
                    Err(err) => {
                        self.fail_scene_import(scene_import, err);
                        return;
                    }
                },
                name: material.name,
            };
            let scene_material = match scene.register_material(scene_data.clone()) {
                Ok(handle) => handle,
                Err(err) => {
                    self.fail_scene_import(scene_import, err.to_string());
                    return;
                }
            };
            scene_materials.push(scene_material);
        }

        let mut spawned_instances = Vec::with_capacity(raw.instances.len());
        for instance in raw.instances {
            let mesh = scene_meshes[instance.mesh_index as usize];
            let materials = instance
                .material_indices
                .into_iter()
                .map(|material_index| scene_materials[material_index as usize])
                .collect();
            let scene_instance = match scene.register_instance(Instance {
                mesh,
                materials,
                transform: instance.transform,
            }) {
                Ok(handle) => handle,
                Err(err) => {
                    self.fail_scene_import(scene_import, err.to_string());
                    return;
                }
            };
            spawned_instances.push(scene_instance);
        }

        let record = self
            .model_imports
            .get_mut(scene_import)
            .expect("SceneAssetIngestor: model import record disappeared during ingest");
        log::info!(
            "SceneAssetIngestor: model {:?} spawned {} runtime instances",
            scene_import,
            spawned_instances.len()
        );
        record.status = LoadStatus::Ready;
        record.error = None;
        record.spawned_instances = Some(spawned_instances);
    }

    fn validate_model_indices(raw: &RawSceneData) -> Result<(), String> {
        for instance in &raw.instances {
            if instance.mesh_index as usize >= raw.meshes.len() {
                return Err(format!(
                    "model instance '{}' references missing mesh {}",
                    instance.name, instance.mesh_index
                ));
            }
            for &material_index in &instance.material_indices {
                if material_index as usize >= raw.materials.len() {
                    return Err(format!(
                        "model instance '{}' references missing material {}",
                        instance.name, material_index
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_model_texture_paths(source_path: &Path, materials: &[RawMaterialData]) -> Result<(), String> {
        for material in materials {
            for (label, path) in [
                ("diffuse", material.diffuse_texture_path.as_ref()),
                ("normal", material.normal_texture_path.as_ref()),
            ] {
                let Some(path) = path else {
                    continue;
                };
                let resolved_path = Self::resolve_scene_texture_path(source_path, path.clone());
                std::fs::canonicalize(&resolved_path).map_err(|err| {
                    format!("failed to canonicalize {label} texture path '{}': {err}", resolved_path.display())
                })?;
            }
        }
        Ok(())
    }

    fn mark_model_failed(&mut self, model_load: ModelLoadHandle, error: String) {
        let scene_import = self.take_scene_import_for_load(model_load);
        self.fail_scene_import(scene_import, error);
    }

    fn fail_scene_import(&mut self, scene_import: ModelImportHandle, error: String) {
        let record = self
            .model_imports
            .get_mut(scene_import)
            .expect("SceneAssetIngestor: model import record disappeared during failure ingest");
        log::error!("SceneAssetIngestor: model {:?} failed: {}", scene_import, error);
        record.status = LoadStatus::Failed;
        record.error = Some(error);
    }

    fn take_scene_import_for_load(&mut self, model_load: ModelLoadHandle) -> ModelImportHandle {
        self.model_loads.remove(model_load).expect("SceneAssetIngestor: received event for unknown model load handle")
    }

    fn take_scene_texture_for_load(&mut self, texture_load: TextureLoadHandle) -> TextureHandle {
        self.texture_loads
            .remove(texture_load)
            .expect("SceneAssetIngestor: received event for unknown texture load handle")
    }

    fn resolve_scene_texture_path(source_path: &Path, texture_path: PathBuf) -> PathBuf {
        let path = if texture_path.is_absolute() {
            texture_path
        } else {
            source_path.parent().unwrap_or_else(|| Path::new("")).join(texture_path)
        };

        path
    }

    fn register_model_texture_ref(
        &mut self,
        assets: &mut AssetHub,
        scene: &mut SceneStore,
        source_path: &Path,
        texture_path: Option<PathBuf>,
        label: &'static str,
    ) -> Result<Option<TextureHandle>, String> {
        let Some(texture_path) = texture_path else {
            return Ok(None);
        };
        let resolved_path = Self::resolve_scene_texture_path(source_path, texture_path);
        let canonical_path = std::fs::canonicalize(&resolved_path).map_err(|err| {
            format!("failed to canonicalize {label} texture path '{}': {err}", resolved_path.display())
        })?;
        Ok(Some(self.register_texture_canonical(assets, scene, canonical_path)))
    }

    fn drain_pending_asset_sync(&mut self, scene: &SceneStore) -> SceneAssetSyncOutput {
        let mut sync = std::mem::take(&mut self.pending_asset_sync);
        sync.pending_texture_uploads.retain(|upload| scene.contains_texture(upload.handle));
        sync.failed_textures.retain(|failed| scene.contains_texture(failed.handle));
        sync.pending_mesh_uploads.retain(|upload| scene.contains_mesh(upload.handle));
        sync
    }
}
