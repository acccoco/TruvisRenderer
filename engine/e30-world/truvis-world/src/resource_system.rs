use std::collections::{HashMap, HashSet};

use slotmap::SlotMap;

use truvis_asset::asset_hub::AssetHub;
use truvis_asset::handle::{MeshData, TextureBytes};

use crate::components::material::MaterialData;
use crate::edit_error::{SceneEditError, SceneHandleKind};
use crate::guid_new_type::{MaterialHandle, MeshHandle, ModelImportHandle, TextureHandle};
use crate::scene_asset_ingestor::SceneAssetIngestor;
use crate::scene_store::SceneStore;

/// CPU ResourceSystem 的资源表。
///
/// 这里保存资源身份、不可变 CPU payload、材质参数和 material -> texture 依赖。
/// 渲染侧扫描最终状态并借用不可变内容提交上传，可以随时从同一个来源重建 GPU 镜像。
#[derive(Default)]
pub(crate) struct ResourceStore {
    /// texture 的 CPU 状态和已解码 payload；像素通过 `TextureBytes` 内部的 `Arc` 共享，
    /// 不会因为交给上传队列而复制大块内存。
    all_textures: SlotMap<TextureHandle, SceneTextureRecord>,
    /// mesh payload 是不可变 CPU 资源；submesh 顺序是 material binding 与 geometry table 的契约。
    all_meshes: SlotMap<MeshHandle, SceneMeshRecord>,
    /// material 参数是 CPU 侧唯一权威内容。
    all_materials: SlotMap<MaterialHandle, SceneMaterialRecord>,
    /// texture -> material 反向依赖，用于查询和删除前检查。
    texture_to_materials: HashMap<TextureHandle, HashSet<MaterialHandle>>,
}

struct SceneTextureRecord {
    state: TextureState,
}

enum TextureState {
    Loading,
    Ready(TextureBytes),
    Failed,
}

struct SceneMeshRecord {
    data: MeshData,
}

impl SceneMeshRecord {
    fn from_mesh_data(data: MeshData) -> Result<Self, SceneEditError> {
        if data.submeshes.is_empty() {
            return Err(SceneEditError::InvalidMeshData {
                reason: format!("mesh '{}' has no submeshes", data.name),
            });
        }

        for (index, submesh) in data.submeshes.iter().enumerate() {
            let count = submesh.positions.len();
            let reason = if count == 0 {
                Some("has no vertices")
            } else if submesh.normals.len() != count || submesh.tangents.len() != count || submesh.uvs.len() != count {
                Some("has mismatched vertex attribute counts")
            } else if submesh.indices.is_empty() || !submesh.indices.len().is_multiple_of(3) {
                Some("must contain triangle indices")
            } else if submesh.indices.iter().any(|&vertex| vertex as usize >= count) {
                Some("has out-of-range vertex indices")
            } else {
                None
            };
            if let Some(reason) = reason {
                return Err(SceneEditError::InvalidMeshData {
                    reason: format!("mesh '{}' submesh {} {}", data.name, index, reason),
                });
            }
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
    /// 只在材质内容真正改变时推进；render-side 以此发现最终状态。
    revision: u64,
}

impl ResourceStore {
    pub(crate) fn contains_texture(&self, handle: TextureHandle) -> bool {
        self.all_textures.contains_key(handle)
    }

    pub(crate) fn contains_mesh(&self, handle: MeshHandle) -> bool {
        self.all_meshes.contains_key(handle)
    }

    pub(crate) fn contains_material(&self, handle: MaterialHandle) -> bool {
        self.all_materials.contains_key(handle)
    }

    pub(crate) fn mesh_submesh_count(&self, handle: MeshHandle) -> Option<usize> {
        self.all_meshes.get(handle).map(SceneMeshRecord::submesh_count)
    }

    pub(crate) fn material_data(&self, handle: MaterialHandle) -> Option<&MaterialData> {
        self.all_materials.get(handle).map(|record| &record.data)
    }

    pub(crate) fn texture_data(&self, handle: TextureHandle) -> Option<&TextureBytes> {
        match &self.all_textures.get(handle)?.state {
            TextureState::Ready(data) => Some(data),
            TextureState::Loading | TextureState::Failed => None,
        }
    }

    pub(crate) fn mesh_data(&self, handle: MeshHandle) -> Option<&MeshData> {
        self.all_meshes.get(handle).map(|record| &record.data)
    }

    pub(crate) fn material_revision(&self, handle: MaterialHandle) -> Option<u64> {
        self.all_materials.get(handle).map(|record| record.revision)
    }

    pub(crate) fn mesh_name(&self, handle: MeshHandle) -> Option<&str> {
        self.all_meshes.get(handle).map(|mesh| mesh.data.name.as_str())
    }

    pub(crate) fn texture_handles(&self) -> impl Iterator<Item = TextureHandle> + '_ {
        self.all_textures.keys()
    }

    pub(crate) fn mesh_handles(&self) -> impl Iterator<Item = MeshHandle> + '_ {
        self.all_meshes.keys()
    }

    pub(crate) fn material_handles(&self) -> impl Iterator<Item = MaterialHandle> + '_ {
        self.all_materials.keys()
    }

    pub(crate) fn materials_using_texture(&self, texture: TextureHandle) -> impl Iterator<Item = MaterialHandle> + '_ {
        self.texture_to_materials
            .get(&texture)
            .into_iter()
            .flat_map(|materials| materials.iter().copied())
    }

    pub(crate) fn register_texture(&mut self) -> TextureHandle {
        self.all_textures.insert(SceneTextureRecord {
            state: TextureState::Loading,
        })
    }

    pub(crate) fn register_mesh(&mut self, data: MeshData) -> Result<MeshHandle, SceneEditError> {
        Ok(self.all_meshes.insert(SceneMeshRecord::from_mesh_data(data)?))
    }

    pub(crate) fn mark_texture_loaded(&mut self, handle: TextureHandle, data: TextureBytes) {
        let Some(texture) = self.all_textures.get_mut(handle) else {
            return;
        };
        texture.state = TextureState::Ready(data);
    }

    pub(crate) fn mark_texture_failed(&mut self, handle: TextureHandle) {
        let Some(texture) = self.all_textures.get_mut(handle) else {
            return;
        };
        texture.state = TextureState::Failed;
    }

    pub(crate) fn register_material(&mut self, data: MaterialData) -> Result<MaterialHandle, SceneEditError> {
        self.validate_material_texture_dependencies(&data)?;
        let handle = self.all_materials.insert(SceneMaterialRecord { data, revision: 1 });
        let data = self.all_materials[handle].data.clone();
        self.add_material_texture_dependencies(handle, &data);
        Ok(handle)
    }

    pub(crate) fn update_material(&mut self, handle: MaterialHandle, data: MaterialData) -> Result<bool, SceneEditError> {
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
        let record = self.all_materials.get_mut(handle).expect("ResourceStore: material disappeared after validation");
        record.data = data;
        record.revision = record.revision.saturating_add(1).max(1);
        Ok(true)
    }

    pub(crate) fn remove_material(&mut self, handle: MaterialHandle, dependent_count: usize) -> Result<(), SceneEditError> {
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

    pub(crate) fn remove_texture(&mut self, handle: TextureHandle, dependent_count: usize) -> Result<(), SceneEditError> {
        if !self.all_textures.contains_key(handle) {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Texture,
            });
        }
        if dependent_count > 0 {
            return Err(SceneEditError::StillReferenced {
                kind: SceneHandleKind::Texture,
                dependent_count,
            });
        }
        self.all_textures.remove(handle);
        self.texture_to_materials.remove(&handle);
        Ok(())
    }

    pub(crate) fn remove_mesh(&mut self, handle: MeshHandle, dependent_count: usize) -> Result<(), SceneEditError> {
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
        for texture in Self::material_texture_handles(data) {
            if !self.all_textures.contains_key(texture) {
                return Err(SceneEditError::MissingDependency {
                    kind: SceneHandleKind::Texture,
                });
            }
        }
        Ok(())
    }

    fn material_texture_handles(data: &MaterialData) -> impl Iterator<Item = TextureHandle> {
        [data.diffuse_texture, data.normal_texture].into_iter().flatten()
    }

    fn add_material_texture_dependencies(&mut self, material: MaterialHandle, data: &MaterialData) {
        for texture in Self::material_texture_handles(data) {
            self.texture_to_materials.entry(texture).or_default().insert(material);
        }
    }

    fn remove_material_texture_dependencies(&mut self, material: MaterialHandle, data: &MaterialData) {
        for texture in Self::material_texture_handles(data) {
            Self::remove_reverse_dependency(&mut self.texture_to_materials, texture, material);
        }
    }

    fn remove_reverse_dependency<K, V>(map: &mut HashMap<K, HashSet<V>>, key: K, value: V)
    where
        K: Eq + std::hash::Hash + Copy,
        V: Eq + std::hash::Hash + Copy,
    {
        let Some(dependents) = map.get_mut(&key) else {
            return;
        };
        dependents.remove(&value);
        if dependents.is_empty() {
            map.remove(&key);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.all_textures.clear();
        self.all_meshes.clear();
        self.all_materials.clear();
        self.texture_to_materials.clear();
    }
}

impl Drop for ResourceStore {
    fn drop(&mut self) {
        log::info!("ResourceStore dropped.");
    }
}

/// CPU 资源的唯一 owner。
///
/// `ResourceSystem` 把资源身份、内容 metadata、loader 和 ingest 状态放在同一边界；
/// 它不创建 Vulkan 对象，也不保存 World 的 instance 关系。Render-side 通过 `SceneReadView`
/// 读取它发布的最终资源状态。
pub struct ResourceSystem {
    pub(crate) store: ResourceStore,
    pub(crate) assets: AssetHub,
    pub(crate) scene_assets: SceneAssetIngestor,
}

impl ResourceSystem {
    pub fn new() -> Self {
        Self {
            store: ResourceStore::default(),
            assets: AssetHub::new(),
            scene_assets: SceneAssetIngestor::new(),
        }
    }

    pub(crate) fn poll_asset_loads(&mut self, scene: &mut SceneStore) {
        let events = self.assets.update();
        self.scene_assets
            .ingest_asset_events(&mut self.assets, &mut self.store, scene, events);
    }

    pub(crate) fn request_model_import(&mut self, path: std::path::PathBuf) -> ModelImportHandle {
        self.scene_assets.request_model_import(&mut self.assets, path)
    }

    pub(crate) fn model_import_status(&self, handle: ModelImportHandle) -> truvis_asset::handle::LoadStatus {
        self.scene_assets.model_import_status(handle)
    }

    pub(crate) fn model_import_error(&self, handle: ModelImportHandle) -> Option<&str> {
        self.scene_assets.model_import_error(handle)
    }

    pub(crate) fn register_texture_canonical(&mut self, path: std::path::PathBuf) -> (TextureHandle, bool) {
        let existing = self
            .scene_assets
            .texture_for_path(&path)
            .filter(|&handle| self.store.contains_texture(handle));
        let handle = self
            .scene_assets
            .register_texture_canonical(&mut self.assets, &mut self.store, path);
        (handle, existing.is_none())
    }

    pub(crate) fn register_mesh(&mut self, data: MeshData) -> Result<MeshHandle, SceneEditError> {
        self.store.register_mesh(data)
    }

    pub(crate) fn register_material(&mut self, data: MaterialData) -> Result<MaterialHandle, SceneEditError> {
        self.store.register_material(data)
    }

    pub(crate) fn update_material(&mut self, handle: MaterialHandle, data: MaterialData) -> Result<bool, SceneEditError> {
        self.store.update_material(handle, data)
    }

    pub(crate) fn remove_material(&mut self, scene: &SceneStore, handle: MaterialHandle) -> Result<(), SceneEditError> {
        let dependent_count = scene.instance_dependents_for_material(handle);
        self.store.remove_material(handle, dependent_count)
    }

    pub(crate) fn remove_texture(
        &mut self,
        scene: &SceneStore,
        handle: TextureHandle,
    ) -> Result<(), SceneEditError> {
        let dependent_count = self.store.materials_using_texture(handle).count() + usize::from(scene.sky_uses_texture(handle));
        self.store.remove_texture(handle, dependent_count)?;
        self.scene_assets.forget_texture(handle);
        Ok(())
    }

    pub(crate) fn remove_mesh(&mut self, scene: &SceneStore, handle: MeshHandle) -> Result<(), SceneEditError> {
        self.store.remove_mesh(handle, scene.instance_dependents_for_mesh(handle))
    }

    pub(crate) fn destroy(mut self) {
        self.store.clear();
        self.assets.destroy();
    }
}

impl Default for ResourceSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::components::material::{CoverageMode, MaterialClass, MaterialData};

    use super::*;

    #[test]
    fn texture_reverse_reference_controls_orphan_removal() {
        let mut resources = ResourceStore::default();
        let texture = resources.register_texture();
        let material = resources
            .register_material(MaterialData {
                base_color: glam::Vec4::ONE,
                metallic: 0.0,
                roughness: 0.5,
                class: MaterialClass::Surface,
                coverage: CoverageMode::Opaque,
                diffuse_texture: Some(texture),
                normal_texture: None,
                name: "textured".to_string(),
            })
            .unwrap();

        assert_eq!(resources.materials_using_texture(texture).collect::<Vec<_>>(), vec![material]);
        assert_eq!(
            resources.remove_texture(texture, 1),
            Err(SceneEditError::StillReferenced {
                kind: SceneHandleKind::Texture,
                dependent_count: 1,
            })
        );

        resources.remove_material(material, 0).unwrap();
        assert!(resources.materials_using_texture(texture).next().is_none());
        resources.remove_texture(texture, 0).unwrap();
        assert!(!resources.contains_texture(texture));
    }
}
