use std::collections::HashMap;
use std::path::{Path, PathBuf};

use slotmap::SlotMap;
use truvis_asset::asset_hub::{AssetHub, AssetLoadEvent};
use truvis_asset::handle::{
    EmbeddedTextureId, LoadStatus, ModelLoadDesc, ModelLoadHandle, RawSceneData, RawTextureSource, TextureColorSpace, TextureLoadDesc,
    TextureLoadHandle,
};
use truvis_asset::material_texture::{TextureChannel, TextureSlot};

use crate::components::instance::Instance;
use crate::components::material::MaterialData;
use crate::guid_new_type::{ModelImportHandle, TextureHandle};
use crate::asset_system::AssetStore;
use crate::scene_store::SceneStore;

/// `GameWorld` 内部的 scene asset ingest 协调器。
///
/// 它是 loader handle 和 CPU world resource handle 的唯一翻译边界。`AssetHub` 只交付一次性 CPU
/// payload；本对象只把 model / texture ingest 到 `AssetStore` 与 `SceneStore`。
#[derive(Default)]
pub struct SceneAssetIngestor {
    model_imports: SlotMap<ModelImportHandle, SceneModelImportRecord>,
    /// 完成事件尚未 ingest 时，AssetHub 已允许复用 task slot；同槽不同 generation
    /// 必须同时保留映射，不能用只容纳一个 generation 的 SecondaryMap。
    model_loads: HashMap<ModelLoadHandle, ModelImportHandle>,
    texture_loads: HashMap<TextureLoadHandle, TextureHandle>,
    texture_paths: HashMap<(PathBuf, TextureColorSpace), TextureHandle>,
    embedded_textures: HashMap<(PathBuf, EmbeddedTextureId, TextureColorSpace), TextureHandle>,
}

struct SceneModelImportRecord {
    status: LoadStatus,
    error: Option<String>,
}

impl SceneAssetIngestor {
    /// 创建空的 scene asset ingest 状态。
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn texture_for_path(&self, path: &Path, color_space: TextureColorSpace) -> Option<TextureHandle> {
        self.texture_paths.get(&(path.to_path_buf(), color_space)).copied()
    }

    pub(crate) fn forget_texture(&mut self, handle: TextureHandle) {
        self.texture_paths.retain(|_, current| *current != handle);
        self.embedded_textures.retain(|_, current| *current != handle);
    }

    /// 提交一次 model import 请求。
    pub fn request_model_import(&mut self, assets: &mut AssetHub, path: PathBuf) -> ModelImportHandle {
        let scene_import = self.model_imports.insert(SceneModelImportRecord {
            status: LoadStatus::Loading,
            error: None,
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
        resources: &mut AssetStore,
        path: PathBuf,
        color_space: TextureColorSpace,
    ) -> TextureHandle {
        let key = (path.clone(), color_space);
        if let Some(&scene_texture) = self.texture_paths.get(&key) {
            if resources.contains_texture(scene_texture) {
                return scene_texture;
            }
        }

        let texture_load = assets.request_texture(TextureLoadDesc::File { path, color_space });
        let scene_texture = resources.register_texture();
        self.texture_loads.insert(texture_load, scene_texture);
        self.texture_paths.insert(key, scene_texture);
        scene_texture
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

    /// 消费 `AssetHub` 完成事件，只更新 CPU registry 和场景最终状态。
    pub fn ingest_asset_events(
        &mut self,
        assets: &mut AssetHub,
        resources: &mut AssetStore,
        scene: &mut SceneStore,
        events: Vec<AssetLoadEvent>,
    ) {
        for event in events {
            self.ingest_asset_event(assets, resources, scene, event);
        }
    }

    fn ingest_asset_event(
        &mut self,
        assets: &mut AssetHub,
        resources: &mut AssetStore,
        scene: &mut SceneStore,
        event: AssetLoadEvent,
    ) {
        match event {
            AssetLoadEvent::TextureLoaded { handle, desc, data } => {
                let scene_texture = self.take_scene_texture_for_load(handle);
                if !resources.contains_texture(scene_texture) {
                    return;
                }
                log::debug!("Texture ready {:?}: {}", scene_texture, desc.source_label());
                resources.mark_texture_loaded(scene_texture, data);
            }
            AssetLoadEvent::TextureFailed { handle, desc, error } => {
                let scene_texture = self.take_scene_texture_for_load(handle);
                if !resources.contains_texture(scene_texture) {
                    return;
                }
                log::error!("Texture load failed {:?} ({}): {}", scene_texture, desc.source_label(), error);
                resources.mark_texture_failed(scene_texture);
            }
            AssetLoadEvent::ModelLoaded { handle, desc: _, data } => {
                self.ingest_model_loaded(assets, resources, scene, handle, data);
            }
            AssetLoadEvent::ModelFailed { handle, desc: _, error } => {
                self.mark_model_failed(handle, error);
            }
        }
    }

    fn ingest_model_loaded(
        &mut self,
        assets: &mut AssetHub,
        resources: &mut AssetStore,
        scene: &mut SceneStore,
        model_load: ModelLoadHandle,
        raw: RawSceneData,
    ) {
        let scene_import = self.take_scene_import_for_load(model_load);
        if let Err(error) = Self::validate_model_payload(&raw) {
            self.fail_scene_import(scene_import, error);
            return;
        }

        let source_path = raw.source_path.clone();
        let mut scene_meshes = Vec::with_capacity(raw.meshes.len());
        for mesh_data in raw.meshes {
            let scene_mesh = match resources.register_mesh(mesh_data) {
                Ok(handle) => handle,
                Err(err) => {
                    self.fail_scene_import(scene_import, err.to_string());
                    return;
                }
            };
            scene_meshes.push(scene_mesh);
        }

        let mut scene_materials = Vec::with_capacity(raw.materials.len());
        for material in raw.materials {
            let mut textures: [Option<TextureSlot<TextureHandle>>; TextureChannel::COUNT] = Default::default();
            for (channel, slot) in TextureChannel::ALL.into_iter().zip(material.textures) {
                let Some(slot) = slot else { continue; };
                match self.register_model_texture_ref(assets, resources, &source_path, slot.texture.clone(), channel) {
                    Ok(texture) => textures[channel as usize] = Some(slot.with_texture(texture)),
                    Err(error) => { self.fail_scene_import(scene_import, error); return; }
                }
            }
            let scene_data = MaterialData {
                base_color: material.base_color,
                metallic: material.metallic,
                roughness: material.roughness,
                class: material.class,
                coverage: material.coverage,
                textures,
                normal_scale: material.normal_scale,
                emissive_factor: material.emissive_factor,
                name: material.name,
            };
            let scene_material = match resources.register_material(scene_data) {
                Ok(handle) => handle,
                Err(err) => {
                    self.fail_scene_import(scene_import, err.to_string());
                    return;
                }
            };
            scene_materials.push(scene_material);
        }

        let instance_count = raw.instances.len();
        for instance in raw.instances {
            let mesh = scene_meshes[instance.mesh_index as usize];
            let materials = instance
                .material_indices
                .into_iter()
                .map(|material_index| scene_materials[material_index as usize])
                .collect();
            if let Err(err) = scene.register_instance(resources, Instance {
                name: instance.name,
                mesh,
                materials,
                transform: instance.transform,
            }) {
                self.fail_scene_import(scene_import, err.to_string());
                return;
            }
        }

        let record = self
            .model_imports
            .get_mut(scene_import)
            .expect("SceneAssetIngestor: model import record disappeared during ingest");
        log::info!(
            "SceneAssetIngestor: model {:?} spawned {} runtime instances",
            scene_import,
            instance_count
        );
        record.status = LoadStatus::Ready;
        record.error = None;
    }

    fn validate_model_payload(raw: &RawSceneData) -> Result<(), String> {
        for mesh in &raw.meshes {
            if mesh.submeshes.is_empty() {
                return Err(format!("model mesh '{}' has no submeshes", mesh.name));
            }
        }

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
            let mesh = &raw.meshes[instance.mesh_index as usize];
            let expected_materials = mesh.submesh_count();
            if instance.material_indices.len() != expected_materials {
                return Err(format!(
                    "model instance '{}' material count mismatch for mesh '{}': expected {}, got {}",
                    instance.name,
                    mesh.name,
                    expected_materials,
                    instance.material_indices.len()
                ));
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
        self.model_loads.remove(&model_load).expect("SceneAssetIngestor: received event for unknown model load handle")
    }

    fn take_scene_texture_for_load(&mut self, texture_load: TextureLoadHandle) -> TextureHandle {
        self.texture_loads
            .remove(&texture_load)
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
        resources: &mut AssetStore,
        source_path: &Path,
        texture_source: RawTextureSource,
        channel: TextureChannel,
    ) -> Result<TextureHandle, String> {
        let color_space = channel.color_space();
        match texture_source {
            RawTextureSource::ExternalPath(texture_path) => {
                let resolved_path = Self::resolve_scene_texture_path(source_path, texture_path);
                let canonical_path = std::fs::canonicalize(&resolved_path).map_err(|err| {
                    format!("failed to canonicalize {} texture path '{}': {err}", channel.name(), resolved_path.display())
                })?;
                Ok(self.register_texture_canonical(assets, resources, canonical_path, color_space))
            }
            RawTextureSource::Embedded {
                identity,
                bytes,
                mime_type,
            } => {
                let key = (source_path.to_path_buf(), identity, color_space);
                if let Some(&handle) = self.embedded_textures.get(&key) {
                    if resources.contains_texture(handle) {
                        return Ok(handle);
                    }
                }
                let handle = resources.register_texture();
                let load = assets.request_texture_bytes(identity, bytes, mime_type, color_space);
                self.texture_loads.insert(load, handle);
                self.embedded_textures.insert(key, handle);
                Ok(handle)
            }
        }
    }

}
