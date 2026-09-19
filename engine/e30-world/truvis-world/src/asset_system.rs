use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use slotmap::SlotMap;

use truvis_asset::asset_load_service::{AssetLoadEvent, AssetLoadService};
use truvis_asset::handle::{
    EmbeddedTextureId, LoadStatus, RawSceneData, RawTextureSource, SceneLoadDesc, SceneLoadHandle, TextureBytes,
    TextureColorSpace, TextureLoadDesc, TextureLoadHandle,
};
use truvis_asset::material_texture::{TextureChannel, TextureSlot};

use crate::components::material::MaterialData;
use crate::edit_error::{SceneEditError, SceneHandleKind};
use crate::guid_new_type::{MaterialAssetHandle, MeshAssetHandle, SceneImportHandle, TextureAssetHandle};

/// 资源的可持久化来源描述。来源不进入资源 handle，而是随 record 保存。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AssetSource {
    File { path: PathBuf },
    Embedded { scene_path: PathBuf, image_index: u32 },
    Generated { key: GeneratedSourceKey },
}

/// 生成资源的稳定去重 key。调用方应把生成器类型和参数编码进字符串。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GeneratedSourceKey(pub String);

/// CPU 纹理记录。`state` 表示 CPU loader 阶段，GPU ready 由 render-side owner 维护。
#[derive(Clone, Debug)]
pub struct TextureRecord {
    pub source: AssetSource,
    pub color_space: TextureColorSpace,
    pub state: TextureState,
    pub data: Option<TextureBytes>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureState {
    Loading,
    Ready,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TextureKey {
    source: AssetSource,
    color_space: TextureColorSpace,
}

/// 已完成资源身份解析的 scene 描述；它不创建 `SceneStore` instance。
#[derive(Clone, Debug)]
pub struct SceneData {
    pub scene_path: PathBuf,
    pub objects: Vec<SceneObjectData>,
}

#[derive(Clone, Debug)]
pub struct SceneObjectData {
    pub name: String,
    pub mesh: MeshAssetHandle,
    pub materials: Vec<MaterialAssetHandle>,
    pub transform: glam::Mat4,
}

struct SceneImportRecord {
    status: LoadStatus,
    error: Option<String>,
    data: Option<SceneData>,
}

#[derive(Default)]
pub(crate) struct AssetStore {
    all_textures: SlotMap<TextureAssetHandle, TextureRecord>,
    texture_by_key: HashMap<TextureKey, TextureAssetHandle>,
    all_meshes: SlotMap<MeshAssetHandle, SceneMeshRecord>,
    all_materials: SlotMap<MaterialAssetHandle, SceneMaterialRecord>,
    texture_to_materials: HashMap<TextureAssetHandle, HashSet<MaterialAssetHandle>>,
}

struct SceneMeshRecord {
    data: truvis_asset::handle::MeshData,
}

impl SceneMeshRecord {
    fn from_mesh_data(data: truvis_asset::handle::MeshData) -> Result<Self, SceneEditError> {
        if data.submeshes.is_empty() {
            return Err(SceneEditError::InvalidMeshData {
                reason: format!("mesh '{}' has no submeshes", data.name),
            });
        }
        for submesh in &data.submeshes {
            submesh.validate().map_err(|reason| SceneEditError::InvalidMeshData { reason })?;
        }
        Ok(Self { data })
    }

    #[inline]
    fn submesh_count(&self) -> usize {
        self.data.submeshes.len()
    }
}

struct SceneMaterialRecord {
    data: MaterialData,
    revision: u64,
}

impl AssetStore {
    pub(crate) fn contains_texture(&self, handle: TextureAssetHandle) -> bool {
        self.all_textures.contains_key(handle)
    }
    pub(crate) fn contains_mesh(&self, handle: MeshAssetHandle) -> bool {
        self.all_meshes.contains_key(handle)
    }
    pub(crate) fn contains_material(&self, handle: MaterialAssetHandle) -> bool {
        self.all_materials.contains_key(handle)
    }
    pub(crate) fn mesh_submesh_count(&self, handle: MeshAssetHandle) -> Option<usize> {
        self.all_meshes.get(handle).map(SceneMeshRecord::submesh_count)
    }
    pub(crate) fn material_data(&self, handle: MaterialAssetHandle) -> Option<&MaterialData> {
        self.all_materials.get(handle).map(|record| &record.data)
    }

    pub(crate) fn texture_data(&self, handle: TextureAssetHandle) -> Option<&TextureBytes> {
        let record = self.all_textures.get(handle)?;
        if record.state == TextureState::Ready { record.data.as_ref() } else { None }
    }

    pub(crate) fn mesh_data(&self, handle: MeshAssetHandle) -> Option<&truvis_asset::handle::MeshData> {
        self.all_meshes.get(handle).map(|record| &record.data)
    }
    pub(crate) fn material_revision(&self, handle: MaterialAssetHandle) -> Option<u64> {
        self.all_materials.get(handle).map(|record| record.revision)
    }
    pub(crate) fn mesh_name(&self, handle: MeshAssetHandle) -> Option<&str> {
        self.all_meshes.get(handle).map(|mesh| mesh.data.name.as_str())
    }
    pub(crate) fn texture_handles(&self) -> impl Iterator<Item = TextureAssetHandle> + '_ {
        self.all_textures.keys()
    }
    pub(crate) fn mesh_handles(&self) -> impl Iterator<Item = MeshAssetHandle> + '_ {
        self.all_meshes.keys()
    }
    pub(crate) fn material_handles(&self) -> impl Iterator<Item = MaterialAssetHandle> + '_ {
        self.all_materials.keys()
    }

    pub(crate) fn materials_using_texture(
        &self,
        texture: TextureAssetHandle,
    ) -> impl Iterator<Item = MaterialAssetHandle> + '_ {
        self.texture_to_materials.get(&texture).into_iter().flat_map(|materials| materials.iter().copied())
    }

    fn find_or_insert_texture(
        &mut self,
        source: AssetSource,
        color_space: TextureColorSpace,
    ) -> (TextureAssetHandle, bool) {
        let key = TextureKey {
            source: source.clone(),
            color_space,
        };
        if let Some(&handle) = self.texture_by_key.get(&key) {
            if self.all_textures.contains_key(handle) {
                return (handle, false);
            }
            self.texture_by_key.remove(&key);
        }
        let handle = self.all_textures.insert(TextureRecord {
            source,
            color_space,
            state: TextureState::Loading,
            data: None,
            error: None,
        });
        self.texture_by_key.insert(key, handle);
        (handle, true)
    }

    pub(crate) fn mark_texture_loaded(&mut self, handle: TextureAssetHandle, data: TextureBytes) {
        if let Some(record) = self.all_textures.get_mut(handle) {
            record.state = TextureState::Ready;
            record.data = Some(data);
            record.error = None;
        }
    }

    pub(crate) fn mark_texture_failed(&mut self, handle: TextureAssetHandle, error: String) {
        if let Some(record) = self.all_textures.get_mut(handle) {
            record.state = TextureState::Failed;
            record.data = None;
            record.error = Some(error);
        }
    }

    pub(crate) fn register_mesh(
        &mut self,
        data: truvis_asset::handle::MeshData,
    ) -> Result<MeshAssetHandle, SceneEditError> {
        Ok(self.all_meshes.insert(SceneMeshRecord::from_mesh_data(data)?))
    }

    pub(crate) fn register_material(&mut self, data: MaterialData) -> Result<MaterialAssetHandle, SceneEditError> {
        self.validate_material_texture_dependencies(&data)?;
        let handle = self.all_materials.insert(SceneMaterialRecord { data, revision: 1 });
        let data = self.all_materials[handle].data.clone();
        self.add_material_texture_dependencies(handle, &data);
        Ok(handle)
    }

    pub(crate) fn update_material(
        &mut self,
        handle: MaterialAssetHandle,
        data: MaterialData,
    ) -> Result<bool, SceneEditError> {
        self.validate_material_texture_dependencies(&data)?;
        let Some(old_data) = self.all_materials.get(handle).map(|record| record.data.clone()) else {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Material,
            });
        };
        if old_data == data {
            return Ok(false);
        }
        self.remove_material_texture_dependencies(handle, &old_data);
        self.add_material_texture_dependencies(handle, &data);
        let record = self.all_materials.get_mut(handle).expect("AssetStore: material disappeared after validation");
        record.data = data;
        record.revision = record.revision.saturating_add(1).max(1);
        Ok(true)
    }

    pub(crate) fn remove_material(
        &mut self,
        handle: MaterialAssetHandle,
        dependent_count: usize,
    ) -> Result<(), SceneEditError> {
        let Some(record) = self.all_materials.get(handle) else {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Material,
            });
        };
        if dependent_count > 0 {
            return Err(SceneEditError::StillReferenced {
                kind: SceneHandleKind::Material,
                dependent_count,
            });
        }
        let data = record.data.clone();
        self.all_materials.remove(handle);
        self.remove_material_texture_dependencies(handle, &data);
        Ok(())
    }

    pub(crate) fn remove_texture(
        &mut self,
        handle: TextureAssetHandle,
        dependent_count: usize,
    ) -> Result<(), SceneEditError> {
        let Some(record) = self.all_textures.get(handle) else {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Texture,
            });
        };
        if dependent_count > 0 {
            return Err(SceneEditError::StillReferenced {
                kind: SceneHandleKind::Texture,
                dependent_count,
            });
        }
        let key = TextureKey {
            source: record.source.clone(),
            color_space: record.color_space,
        };
        self.all_textures.remove(handle);
        self.texture_by_key.remove(&key);
        self.texture_to_materials.remove(&handle);
        Ok(())
    }

    pub(crate) fn remove_mesh(
        &mut self,
        handle: MeshAssetHandle,
        dependent_count: usize,
    ) -> Result<(), SceneEditError> {
        if !self.all_meshes.contains_key(handle) {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Mesh,
            });
        }
        if dependent_count > 0 {
            return Err(SceneEditError::StillReferenced {
                kind: SceneHandleKind::Mesh,
                dependent_count,
            });
        }
        self.all_meshes.remove(handle);
        Ok(())
    }

    fn validate_material_texture_dependencies(&self, data: &MaterialData) -> Result<(), SceneEditError> {
        data.validate().map_err(|reason| SceneEditError::InvalidMaterialData { reason })?;
        for texture in data.textures.iter().flatten().map(|slot| slot.texture) {
            if !self.all_textures.contains_key(texture) {
                return Err(SceneEditError::MissingDependency {
                    kind: SceneHandleKind::Texture,
                });
            }
        }
        Ok(())
    }

    fn add_material_texture_dependencies(&mut self, material: MaterialAssetHandle, data: &MaterialData) {
        for texture in data.textures.iter().flatten().map(|slot| slot.texture) {
            self.texture_to_materials.entry(texture).or_default().insert(material);
        }
    }

    fn remove_material_texture_dependencies(&mut self, material: MaterialAssetHandle, data: &MaterialData) {
        for texture in data.textures.iter().flatten().map(|slot| slot.texture) {
            let Some(dependents) = self.texture_to_materials.get_mut(&texture) else {
                continue;
            };
            dependents.remove(&material);
            if dependents.is_empty() {
                self.texture_to_materials.remove(&texture);
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        self.all_textures.clear();
        self.texture_by_key.clear();
        self.all_meshes.clear();
        self.all_materials.clear();
        self.texture_to_materials.clear();
    }
}

/// CPU 资源和 scene import 的唯一 owner。它不保存 `SceneStore`，也不创建 runtime instance。
pub struct AssetSystem {
    pub(crate) store: AssetStore,
    load_service: AssetLoadService,
    scene_imports: SlotMap<SceneImportHandle, SceneImportRecord>,
    scene_loads: HashMap<SceneLoadHandle, SceneImportHandle>,
    texture_loads: HashMap<TextureLoadHandle, TextureAssetHandle>,
}

impl AssetSystem {
    pub fn new() -> Self {
        Self {
            store: AssetStore::default(),
            load_service: AssetLoadService::new(),
            scene_imports: SlotMap::with_key(),
            scene_loads: HashMap::new(),
            texture_loads: HashMap::new(),
        }
    }

    pub(crate) fn poll_asset_loads(&mut self) {
        for event in self.load_service.update() {
            self.ingest_asset_event(event);
        }
    }

    pub(crate) fn import_texture_file(
        &mut self,
        path: PathBuf,
        color_space: TextureColorSpace,
    ) -> Result<(TextureAssetHandle, bool), String> {
        let path = std::fs::canonicalize(&path)
            .map_err(|err| format!("failed to canonicalize texture path '{}': {err}", path.display()))?;
        Ok(self.register_texture_source(
            AssetSource::File { path: path.clone() },
            color_space,
            Some(TextureLoadDesc::File { path, color_space }),
        ))
    }

    fn register_texture_source(
        &mut self,
        source: AssetSource,
        color_space: TextureColorSpace,
        desc: Option<TextureLoadDesc>,
    ) -> (TextureAssetHandle, bool) {
        let (handle, is_new) = self.store.find_or_insert_texture(source, color_space);
        if is_new {
            if let Some(desc) = desc {
                let load = self.load_service.request_texture(desc);
                self.texture_loads.insert(load, handle);
            }
        }
        (handle, is_new)
    }

    pub(crate) fn import_scene(&mut self, path: PathBuf) -> SceneImportHandle {
        let scene_import = self.scene_imports.insert(SceneImportRecord {
            status: LoadStatus::Loading,
            error: None,
            data: None,
        });
        let path = match std::fs::canonicalize(&path) {
            Ok(path) => path,
            Err(err) => {
                self.fail_scene_import(scene_import, format!("failed to canonicalize scene path: {err}"));
                return scene_import;
            }
        };
        let scene_load = self.load_service.request_scene(SceneLoadDesc { path });
        self.scene_loads.insert(scene_load, scene_import);
        scene_import
    }

    pub(crate) fn scene_import_status(&self, handle: SceneImportHandle) -> LoadStatus {
        self.scene_imports.get(handle).map_or(LoadStatus::Failed, |record| record.status)
    }
    pub(crate) fn scene_import_error(&self, handle: SceneImportHandle) -> Option<&str> {
        self.scene_imports.get(handle).and_then(|record| record.error.as_deref())
    }
    pub(crate) fn scene_data(&self, handle: SceneImportHandle) -> Option<&SceneData> {
        self.scene_imports.get(handle).and_then(|record| record.data.as_ref())
    }
    pub(crate) fn register_mesh(
        &mut self,
        data: truvis_asset::handle::MeshData,
    ) -> Result<MeshAssetHandle, SceneEditError> {
        self.store.register_mesh(data)
    }
    pub(crate) fn register_material(&mut self, data: MaterialData) -> Result<MaterialAssetHandle, SceneEditError> {
        self.store.register_material(data)
    }
    pub(crate) fn update_material(
        &mut self,
        handle: MaterialAssetHandle,
        data: MaterialData,
    ) -> Result<bool, SceneEditError> {
        self.store.update_material(handle, data)
    }
    pub(crate) fn remove_material(
        &mut self,
        handle: MaterialAssetHandle,
        dependent_count: usize,
    ) -> Result<(), SceneEditError> {
        self.store.remove_material(handle, dependent_count)
    }
    pub(crate) fn remove_texture(
        &mut self,
        handle: TextureAssetHandle,
        dependent_count: usize,
    ) -> Result<(), SceneEditError> {
        self.store.remove_texture(handle, dependent_count)
    }
    pub(crate) fn remove_mesh(
        &mut self,
        handle: MeshAssetHandle,
        dependent_count: usize,
    ) -> Result<(), SceneEditError> {
        self.store.remove_mesh(handle, dependent_count)
    }

    pub(crate) fn destroy(mut self) {
        self.store.clear();
        self.load_service.destroy();
    }

    fn ingest_asset_event(&mut self, event: AssetLoadEvent) {
        match event {
            AssetLoadEvent::TextureLoaded { handle, desc, data } => {
                let Some(texture) = self.texture_loads.remove(&handle) else {
                    log::error!("AssetSystem: unknown texture load handle {handle:?}");
                    return;
                };
                if self.store.contains_texture(texture) {
                    log::debug!("Texture ready {:?}: {}", texture, desc.source_label());
                    self.store.mark_texture_loaded(texture, data);
                }
            }
            AssetLoadEvent::TextureFailed { handle, desc, error } => {
                let Some(texture) = self.texture_loads.remove(&handle) else {
                    log::error!("AssetSystem: unknown texture load handle {handle:?}");
                    return;
                };
                if self.store.contains_texture(texture) {
                    log::error!("Texture load failed {:?} ({}): {}", texture, desc.source_label(), error);
                    self.store.mark_texture_failed(texture, error);
                }
            }
            AssetLoadEvent::SceneLoaded { handle, desc, data } => self.ingest_scene_loaded(handle, desc.path, data),
            AssetLoadEvent::SceneFailed { handle, error, .. } => {
                let scene_import = self.take_scene_import_for_load(handle);
                self.fail_scene_import(scene_import, error);
            }
        }
    }

    fn ingest_scene_loaded(&mut self, scene_load: SceneLoadHandle, scene_path: PathBuf, raw: RawSceneData) {
        let scene_import = self.take_scene_import_for_load(scene_load);
        if let Err(error) = Self::validate_scene_payload(&raw) {
            self.fail_scene_import(scene_import, error);
            return;
        }
        let source_path = scene_path;
        let mut meshes = Vec::with_capacity(raw.meshes.len());
        for mesh in raw.meshes {
            match self.register_mesh(mesh) {
                Ok(handle) => meshes.push(handle),
                Err(error) => {
                    self.fail_scene_import(scene_import, error.to_string());
                    return;
                }
            }
        }
        let mut materials = Vec::with_capacity(raw.materials.len());
        for material in raw.materials {
            let mut textures: [Option<TextureSlot<TextureAssetHandle>>; TextureChannel::COUNT] = Default::default();
            for (channel, slot) in TextureChannel::ALL.into_iter().zip(material.textures) {
                let Some(slot) = slot else {
                    continue;
                };
                match self.register_scene_texture(&source_path, slot.texture.clone(), channel) {
                    Ok(texture) => textures[channel as usize] = Some(slot.with_texture(texture)),
                    Err(error) => {
                        self.fail_scene_import(scene_import, error);
                        return;
                    }
                }
            }
            let data = MaterialData {
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
            match self.register_material(data) {
                Ok(handle) => materials.push(handle),
                Err(error) => {
                    self.fail_scene_import(scene_import, error.to_string());
                    return;
                }
            }
        }
        let objects = raw
            .instances
            .into_iter()
            .map(|instance| SceneObjectData {
                name: instance.name,
                mesh: meshes[instance.mesh_index as usize],
                materials: instance.material_indices.into_iter().map(|index| materials[index as usize]).collect(),
                transform: instance.transform,
            })
            .collect();
        let record =
            self.scene_imports.get_mut(scene_import).expect("AssetSystem: scene import disappeared during ingest");
        record.status = LoadStatus::Ready;
        record.error = None;
        record.data = Some(SceneData {
            scene_path: source_path,
            objects,
        });
    }

    fn validate_scene_payload(raw: &RawSceneData) -> Result<(), String> {
        for mesh in &raw.meshes {
            if mesh.submeshes.is_empty() {
                return Err(format!("scene mesh '{}' has no submeshes", mesh.name));
            }
        }
        for instance in &raw.instances {
            if instance.mesh_index as usize >= raw.meshes.len() {
                return Err(format!(
                    "scene instance '{}' references missing mesh {}",
                    instance.name, instance.mesh_index
                ));
            }
            for &material_index in &instance.material_indices {
                if material_index as usize >= raw.materials.len() {
                    return Err(format!(
                        "scene instance '{}' references missing material {}",
                        instance.name, material_index
                    ));
                }
            }
            let expected = raw.meshes[instance.mesh_index as usize].submesh_count();
            if instance.material_indices.len() != expected {
                return Err(format!(
                    "scene instance '{}' material count mismatch: expected {}, got {}",
                    instance.name,
                    expected,
                    instance.material_indices.len()
                ));
            }
        }
        Ok(())
    }

    fn register_scene_texture(
        &mut self,
        scene_path: &Path,
        source: RawTextureSource,
        channel: TextureChannel,
    ) -> Result<TextureAssetHandle, String> {
        let color_space = channel.color_space();
        match source {
            RawTextureSource::ExternalPath(path) => {
                let resolved = if path.is_absolute() {
                    path
                } else {
                    scene_path.parent().unwrap_or_else(|| Path::new("")).join(path)
                };
                let canonical = std::fs::canonicalize(&resolved).map_err(|err| {
                    format!("failed to canonicalize {} texture path '{}': {err}", channel.name(), resolved.display())
                })?;
                Ok(self
                    .register_texture_source(
                        AssetSource::File {
                            path: canonical.clone(),
                        },
                        color_space,
                        Some(TextureLoadDesc::File {
                            path: canonical,
                            color_space,
                        }),
                    )
                    .0)
            }
            RawTextureSource::Embedded {
                identity: EmbeddedTextureId { image_index },
                bytes,
                mime_type,
            } => {
                let scene_path = std::fs::canonicalize(scene_path).map_err(|err| {
                    format!("failed to canonicalize embedded scene path '{}': {err}", scene_path.display())
                })?;
                Ok(self
                    .register_texture_source(
                        AssetSource::Embedded {
                            scene_path,
                            image_index,
                        },
                        color_space,
                        Some(TextureLoadDesc::Embedded {
                            identity: EmbeddedTextureId { image_index },
                            bytes,
                            mime_type,
                            color_space,
                        }),
                    )
                    .0)
            }
        }
    }

    fn take_scene_import_for_load(&mut self, load: SceneLoadHandle) -> SceneImportHandle {
        self.scene_loads.remove(&load).expect("AssetSystem: received event for unknown scene load handle")
    }

    fn fail_scene_import(&mut self, handle: SceneImportHandle, error: String) {
        let record = self.scene_imports.get_mut(handle).expect("AssetSystem: scene import record disappeared");
        record.status = LoadStatus::Failed;
        record.error = Some(error.clone());
        record.data = None;
        log::error!("AssetSystem: scene {:?} failed: {}", handle, error);
    }
}

impl Default for AssetSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::material::{CoverageMode, MaterialClass};

    #[test]
    fn texture_reverse_reference_controls_orphan_removal() {
        let mut resources = AssetStore::default();
        let texture = resources
            .find_or_insert_texture(
                AssetSource::Generated {
                    key: GeneratedSourceKey("test".into()),
                },
                TextureColorSpace::Linear,
            )
            .0;
        let material = resources
            .register_material(MaterialData {
                base_color: glam::Vec4::ONE,
                metallic: 0.0,
                roughness: 0.5,
                class: MaterialClass::Surface,
                coverage: CoverageMode::Opaque,
                textures: [Some(TextureSlot::new(texture)), None, None, None],
                name: "textured".to_string(),
                ..MaterialData::default()
            })
            .unwrap();
        assert_eq!(resources.materials_using_texture(texture).collect::<Vec<_>>(), vec![material]);
        assert_eq!(
            resources.remove_texture(texture, 1),
            Err(SceneEditError::StillReferenced {
                kind: SceneHandleKind::Texture,
                dependent_count: 1
            })
        );
        resources.remove_material(material, 0).unwrap();
        resources.remove_texture(texture, 0).unwrap();
        assert!(!resources.contains_texture(texture));
    }
}
