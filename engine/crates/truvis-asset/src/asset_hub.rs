use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use slotmap::SlotMap;

use crate::asset_loader::{AssetLoadRequest, AssetLoader, LoadResult};
use crate::handle::{
    AssetMaterialHandle, AssetMeshHandle, AssetTextureHandle, LoadStatus, LoadedMaterialData, LoadedMeshData,
    LoadedTextureBytes, MaterialAssetKey, MeshAssetKey,
};

pub struct TextureAssetRecord {
    pub path: PathBuf,
    pub status: LoadStatus,
}

pub struct MeshAssetRecord {
    pub key: MeshAssetKey,
    pub status: LoadStatus,
    pub data: LoadedMeshData,
}

pub struct MaterialAssetRecord {
    pub key: MaterialAssetKey,
    pub status: LoadStatus,
    pub data: LoadedMaterialData,
}

pub enum LoadedAssetEvent {
    TextureLoaded {
        handle: AssetTextureHandle,
        data: LoadedTextureBytes,
    },
    TextureFailed {
        handle: AssetTextureHandle,
        error: String,
    },
    MeshLoaded {
        handle: AssetMeshHandle,
        data: LoadedMeshData,
    },
}

/// 资产中心。
///
/// 只负责资产身份、路径去重和文件到 CPU bytes 的加载流程。
pub struct AssetHub {
    textures: SlotMap<AssetTextureHandle, TextureAssetRecord>,
    meshes: SlotMap<AssetMeshHandle, MeshAssetRecord>,
    materials: SlotMap<AssetMaterialHandle, MaterialAssetRecord>,
    path_to_texture: HashMap<PathBuf, AssetTextureHandle>,
    key_to_mesh: HashMap<MeshAssetKey, AssetMeshHandle>,
    key_to_material: HashMap<MaterialAssetKey, AssetMaterialHandle>,
    pending_events: VecDeque<LoadedAssetEvent>,
    loader: AssetLoader,
}

impl Default for AssetHub {
    fn default() -> Self {
        Self::new()
    }
}

// 创建与初始化
impl AssetHub {
    pub fn new() -> Self {
        let _span = tracy_client::span!("AssetHub::new");

        Self {
            textures: SlotMap::with_key(),
            meshes: SlotMap::with_key(),
            materials: SlotMap::with_key(),
            path_to_texture: HashMap::new(),
            key_to_mesh: HashMap::new(),
            key_to_material: HashMap::new(),
            pending_events: VecDeque::new(),
            loader: AssetLoader::new(),
        }
    }
}

// 销毁
impl AssetHub {
    pub fn destroy(self) {}
}

// 访问器
impl AssetHub {
    /// 请求加载纹理。
    ///
    /// 同一路径只分配一个稳定的 `AssetTextureHandle`。
    pub fn load_texture(&mut self, path: PathBuf) -> AssetTextureHandle {
        let _span = tracy_client::span!("AssetHub::load_texture");
        if let Some(&handle) = self.path_to_texture.get(&path) {
            return handle;
        }

        let handle = self.textures.insert(TextureAssetRecord {
            path: path.clone(),
            status: LoadStatus::Loading,
        });
        self.path_to_texture.insert(path.clone(), handle);

        log::info!("Request load texture: {:?}", path);
        self.loader.request_load(AssetLoadRequest { path, handle });

        handle
    }

    pub fn get_status(&self, handle: AssetTextureHandle) -> LoadStatus {
        self.textures.get(handle).map(|record| record.status).unwrap_or(LoadStatus::Failed)
    }

    pub fn get_mesh_status(&self, handle: AssetMeshHandle) -> LoadStatus {
        self.meshes.get(handle).map(|record| record.status).unwrap_or(LoadStatus::Failed)
    }

    pub fn get_material_status(&self, handle: AssetMaterialHandle) -> LoadStatus {
        self.materials.get(handle).map(|record| record.status).unwrap_or(LoadStatus::Failed)
    }

    pub fn texture_handle_by_path(&self, path: &Path) -> Option<AssetTextureHandle> {
        self.path_to_texture.get(path).copied()
    }

    pub fn mesh_handle_by_key(&self, key: &MeshAssetKey) -> Option<AssetMeshHandle> {
        self.key_to_mesh.get(key).copied()
    }

    pub fn material_handle_by_key(&self, key: &MaterialAssetKey) -> Option<AssetMaterialHandle> {
        self.key_to_material.get(key).copied()
    }

    pub fn get_material_data(&self, handle: AssetMaterialHandle) -> Option<&LoadedMaterialData> {
        self.materials.get(handle).map(|record| &record.data)
    }

    pub fn iter_materials(&self) -> impl Iterator<Item = (AssetMaterialHandle, &LoadedMaterialData)> + '_ {
        self.materials.iter().map(|(handle, record)| (handle, &record.data))
    }

    /// 注册已经位于 CPU 内存中的 mesh 数据。
    ///
    /// 这通常由同步导入器或未来的后台 scene loader 调用；同一个 key 只会产出一次
    /// `MeshLoaded` 事件，GPU 上传由 render-side uploader 消费事件后完成。
    pub fn register_mesh_data(&mut self, key: MeshAssetKey, data: LoadedMeshData) -> AssetMeshHandle {
        let _span = tracy_client::span!("AssetHub::register_mesh_data");
        if let Some(&handle) = self.key_to_mesh.get(&key) {
            return handle;
        }

        let handle = self.meshes.insert(MeshAssetRecord {
            key: key.clone(),
            status: LoadStatus::Ready,
            data: data.clone(),
        });
        self.key_to_mesh.insert(key, handle);
        self.pending_events.push_back(LoadedAssetEvent::MeshLoaded { handle, data });
        handle
    }

    /// 注册已经位于 CPU 内存中的 material 数据。
    ///
    /// GPU material slot 由 render-side `MaterialBridge` 分配，`AssetHub` 只保存内容身份和参数。
    pub fn register_material_data(&mut self, key: MaterialAssetKey, data: LoadedMaterialData) -> AssetMaterialHandle {
        let _span = tracy_client::span!("AssetHub::register_material_data");
        if let Some(&handle) = self.key_to_material.get(&key) {
            return handle;
        }

        let handle = self.materials.insert(MaterialAssetRecord {
            key: key.clone(),
            status: LoadStatus::Ready,
            data,
        });
        self.key_to_material.insert(key, handle);
        handle
    }

    /// 收集后台加载任务完成事件。
    pub fn update(&mut self) -> Vec<LoadedAssetEvent> {
        let _span = tracy_client::span!("AssetHub::update");
        let mut events = Vec::new();

        while let Some(event) = self.pending_events.pop_front() {
            events.push(event);
        }

        while let Some(result) = self.loader.try_recv_result() {
            match result {
                LoadResult::Success { handle, data } => {
                    if let Some(record) = self.textures.get_mut(handle) {
                        record.status = LoadStatus::Ready;
                    }

                    events.push(LoadedAssetEvent::TextureLoaded { handle, data });
                }
                LoadResult::Failure(handle, error) => {
                    if let Some(record) = self.textures.get_mut(handle) {
                        record.status = LoadStatus::Failed;
                    }

                    events.push(LoadedAssetEvent::TextureFailed { handle, error });
                }
            }
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh_key() -> MeshAssetKey {
        MeshAssetKey {
            source_path: PathBuf::from("assets/model.fbx"),
            mesh_index: 7,
        }
    }

    fn mesh_data(name: &str) -> LoadedMeshData {
        LoadedMeshData {
            positions: vec![glam::Vec3::ZERO, glam::Vec3::X, glam::Vec3::Y],
            normals: vec![glam::Vec3::Z; 3],
            tangents: vec![glam::Vec3::X; 3],
            uvs: vec![glam::Vec2::ZERO; 3],
            indices: vec![0, 1, 2],
            name: name.to_string(),
        }
    }

    fn material_key() -> MaterialAssetKey {
        MaterialAssetKey {
            source_path: PathBuf::from("assets/model.fbx"),
            material_index: 3,
        }
    }

    fn material_data(name: &str) -> LoadedMaterialData {
        LoadedMaterialData {
            base_color: glam::Vec4::ONE,
            emissive: glam::Vec4::ZERO,
            metallic: 0.1,
            roughness: 0.6,
            opaque: 1.0,
            diffuse_texture: None,
            normal_texture: None,
            name: name.to_string(),
        }
    }

    #[test]
    fn register_mesh_data_deduplicates_by_key() {
        let mut hub = AssetHub::new();
        let key = mesh_key();

        let first = hub.register_mesh_data(key.clone(), mesh_data("first"));
        let second = hub.register_mesh_data(key, mesh_data("second"));

        assert_eq!(first, second);
        assert_eq!(hub.get_mesh_status(first), LoadStatus::Ready);
        assert_eq!(hub.update().len(), 1);
        assert!(hub.update().is_empty());
    }

    #[test]
    fn register_mesh_data_emits_loaded_event_once() {
        let mut hub = AssetHub::new();
        let key = mesh_key();
        let handle = hub.register_mesh_data(key, mesh_data("mesh"));

        let events = hub.update();
        assert_eq!(events.len(), 1);
        match &events[0] {
            LoadedAssetEvent::MeshLoaded {
                handle: event_handle,
                data,
            } => {
                assert_eq!(*event_handle, handle);
                assert_eq!(data.name, "mesh");
            }
            _ => panic!("expected mesh loaded event"),
        }
    }

    #[test]
    fn register_material_data_deduplicates_by_key() {
        let mut hub = AssetHub::new();
        let key = material_key();

        let first = hub.register_material_data(key.clone(), material_data("first"));
        let second = hub.register_material_data(key, material_data("second"));

        assert_eq!(first, second);
        assert_eq!(hub.get_material_status(first), LoadStatus::Ready);
        assert_eq!(hub.get_material_data(first).unwrap().name, "first");
    }

    #[test]
    fn register_material_data_can_be_iterated() {
        let mut hub = AssetHub::new();
        let handle = hub.register_material_data(material_key(), material_data("mat"));

        let materials = hub.iter_materials().collect::<Vec<_>>();

        assert_eq!(materials.len(), 1);
        assert_eq!(materials[0].0, handle);
        assert_eq!(materials[0].1.name, "mat");
    }
}
