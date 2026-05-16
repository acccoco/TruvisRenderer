use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use slotmap::SlotMap;

use crate::asset_loader::{AssetLoadRequest, AssetLoader, LoadResult, SceneLoadRequest};
use crate::handle::{
    AssetMaterialHandle, AssetMeshHandle, AssetSceneHandle, AssetTextureHandle, LoadStatus, LoadedMaterialData,
    LoadedMeshData, LoadedSceneData, LoadedSceneInstanceData, LoadedTextureBytes, MaterialAssetKey, MeshAssetKey,
    RawLoadedMaterialData, RawLoadedSceneData, SceneAssetKey,
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

pub struct SceneAssetRecord {
    pub key: SceneAssetKey,
    pub status: LoadStatus,
    pub data: Option<LoadedSceneData>,
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
    SceneLoaded {
        handle: AssetSceneHandle,
    },
    SceneFailed {
        handle: AssetSceneHandle,
        error: String,
    },
}

/// 资产中心。
///
/// 只负责资产身份、路径去重和文件到 CPU bytes 的加载流程。
pub struct AssetHub {
    textures: SlotMap<AssetTextureHandle, TextureAssetRecord>,
    meshes: SlotMap<AssetMeshHandle, MeshAssetRecord>,
    materials: SlotMap<AssetMaterialHandle, MaterialAssetRecord>,
    scenes: SlotMap<AssetSceneHandle, SceneAssetRecord>,
    path_to_texture: HashMap<PathBuf, AssetTextureHandle>,
    key_to_mesh: HashMap<MeshAssetKey, AssetMeshHandle>,
    key_to_material: HashMap<MaterialAssetKey, AssetMaterialHandle>,
    key_to_scene: HashMap<SceneAssetKey, AssetSceneHandle>,
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
            scenes: SlotMap::with_key(),
            path_to_texture: HashMap::new(),
            key_to_mesh: HashMap::new(),
            key_to_material: HashMap::new(),
            key_to_scene: HashMap::new(),
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

    /// 请求后台导入 scene / prefab。
    ///
    /// 返回的 handle 只代表 CPU scene asset；runtime instance 需要在 scene ready 后显式 spawn。
    pub fn load_scene(&mut self, path: PathBuf) -> AssetSceneHandle {
        let _span = tracy_client::span!("AssetHub::load_scene");
        let key = SceneAssetKey {
            source_path: path.clone(),
        };
        if let Some(&handle) = self.key_to_scene.get(&key) {
            return handle;
        }

        let handle = self.scenes.insert(SceneAssetRecord {
            key: key.clone(),
            status: LoadStatus::Loading,
            data: None,
        });
        self.key_to_scene.insert(key, handle);

        log::info!("Request load scene: {:?}", path);
        self.loader.request_load_scene(SceneLoadRequest { path, handle });

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

    pub fn get_scene_status(&self, handle: AssetSceneHandle) -> LoadStatus {
        self.scenes.get(handle).map(|record| record.status).unwrap_or(LoadStatus::Failed)
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

    pub fn scene_handle_by_key(&self, key: &SceneAssetKey) -> Option<AssetSceneHandle> {
        self.key_to_scene.get(key).copied()
    }

    pub fn get_material_data(&self, handle: AssetMaterialHandle) -> Option<&LoadedMaterialData> {
        self.materials.get(handle).map(|record| &record.data)
    }

    pub fn get_scene_data(&self, handle: AssetSceneHandle) -> Option<&LoadedSceneData> {
        self.scenes.get(handle).and_then(|record| record.data.as_ref())
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
        let (handle, event) = self.register_mesh_data_inner(key, data);
        if let Some(event) = event {
            self.pending_events.push_back(event);
        }
        handle
    }

    /// 注册已经位于 CPU 内存中的 material 数据。
    ///
    /// GPU material slot 由 render-side `MaterialBridge` 分配，`AssetHub` 只保存内容身份和参数。
    pub fn register_material_data(&mut self, key: MaterialAssetKey, data: LoadedMaterialData) -> AssetMaterialHandle {
        let _span = tracy_client::span!("AssetHub::register_material_data");
        self.register_material_data_inner(key, data)
    }

    fn register_mesh_data_inner(
        &mut self,
        key: MeshAssetKey,
        data: LoadedMeshData,
    ) -> (AssetMeshHandle, Option<LoadedAssetEvent>) {
        if let Some(&handle) = self.key_to_mesh.get(&key) {
            return (handle, None);
        }

        let handle = self.meshes.insert(MeshAssetRecord {
            key: key.clone(),
            status: LoadStatus::Ready,
            data: data.clone(),
        });
        self.key_to_mesh.insert(key, handle);
        (handle, Some(LoadedAssetEvent::MeshLoaded { handle, data }))
    }

    fn register_material_data_inner(&mut self, key: MaterialAssetKey, data: LoadedMaterialData) -> AssetMaterialHandle {
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

    fn material_data_from_raw(&mut self, raw: RawLoadedMaterialData) -> LoadedMaterialData {
        LoadedMaterialData {
            base_color: raw.base_color,
            emissive: raw.emissive,
            metallic: raw.metallic,
            roughness: raw.roughness,
            opaque: raw.opaque,
            diffuse_texture: raw.diffuse_texture_path.map(|path| self.load_texture(path)),
            normal_texture: raw.normal_texture_path.map(|path| self.load_texture(path)),
            name: raw.name,
        }
    }

    fn ingest_loaded_scene(
        &mut self,
        raw: RawLoadedSceneData,
    ) -> Result<(LoadedSceneData, Vec<LoadedAssetEvent>), String> {
        let source_path = raw.source_path;
        let mut immediate_events = Vec::new();

        let mut mesh_handles = Vec::with_capacity(raw.meshes.len());
        for (mesh_index, mesh_data) in raw.meshes.into_iter().enumerate() {
            let (handle, event) = self.register_mesh_data_inner(
                MeshAssetKey {
                    source_path: source_path.clone(),
                    mesh_index: mesh_index as u32,
                },
                mesh_data,
            );
            mesh_handles.push(handle);
            if let Some(event) = event {
                immediate_events.push(event);
            }
        }

        let mut material_handles = Vec::with_capacity(raw.materials.len());
        for (material_index, material_data) in raw.materials.into_iter().enumerate() {
            let data = self.material_data_from_raw(material_data);
            let handle = self.register_material_data_inner(
                MaterialAssetKey {
                    source_path: source_path.clone(),
                    material_index: material_index as u32,
                },
                data,
            );
            material_handles.push(handle);
        }

        let mut instances = Vec::with_capacity(raw.instances.len());
        for instance in raw.instances {
            let mesh = mesh_handles
                .get(instance.mesh_index as usize)
                .copied()
                .ok_or_else(|| format!("scene instance references missing mesh {}", instance.mesh_index))?;
            let mut materials = Vec::with_capacity(instance.material_indices.len());
            for material_index in instance.material_indices {
                let material = material_handles
                    .get(material_index as usize)
                    .copied()
                    .ok_or_else(|| format!("scene instance references missing material {}", material_index))?;
                materials.push(material);
            }

            instances.push(LoadedSceneInstanceData {
                mesh,
                materials,
                transform: instance.transform,
                name: instance.name,
            });
        }

        Ok((
            LoadedSceneData {
                source_path,
                name: raw.name,
                meshes: mesh_handles,
                materials: material_handles,
                instances,
            },
            immediate_events,
        ))
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
                LoadResult::TextureSuccess { handle, data } => {
                    if let Some(record) = self.textures.get_mut(handle) {
                        record.status = LoadStatus::Ready;
                    }

                    events.push(LoadedAssetEvent::TextureLoaded { handle, data });
                }
                LoadResult::TextureFailure(handle, error) => {
                    if let Some(record) = self.textures.get_mut(handle) {
                        record.status = LoadStatus::Failed;
                    }

                    events.push(LoadedAssetEvent::TextureFailed { handle, error });
                }
                LoadResult::SceneSuccess { handle, data } => match self.ingest_loaded_scene(data) {
                    Ok((scene_data, mut scene_events)) => {
                        if let Some(record) = self.scenes.get_mut(handle) {
                            record.status = LoadStatus::Ready;
                            record.data = Some(scene_data);
                        }
                        events.append(&mut scene_events);
                        events.push(LoadedAssetEvent::SceneLoaded { handle });
                    }
                    Err(error) => {
                        if let Some(record) = self.scenes.get_mut(handle) {
                            record.status = LoadStatus::Failed;
                        }
                        events.push(LoadedAssetEvent::SceneFailed { handle, error });
                    }
                },
                LoadResult::SceneFailure(handle, error) => {
                    if let Some(record) = self.scenes.get_mut(handle) {
                        record.status = LoadStatus::Failed;
                    }

                    events.push(LoadedAssetEvent::SceneFailed { handle, error });
                }
            }
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handle::RawLoadedSceneInstanceData;

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

    fn raw_scene_data() -> RawLoadedSceneData {
        RawLoadedSceneData {
            source_path: PathBuf::from("assets/model.fbx"),
            name: "model.fbx".to_string(),
            meshes: vec![mesh_data("mesh")],
            materials: vec![RawLoadedMaterialData {
                base_color: glam::Vec4::ONE,
                emissive: glam::Vec4::ZERO,
                metallic: 0.0,
                roughness: 1.0,
                opaque: 1.0,
                diffuse_texture_path: None,
                normal_texture_path: None,
                name: "mat".to_string(),
            }],
            instances: vec![RawLoadedSceneInstanceData {
                mesh_index: 0,
                material_indices: vec![0],
                transform: glam::Mat4::IDENTITY,
                name: "instance".to_string(),
            }],
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

    #[test]
    fn ingest_loaded_scene_registers_internal_asset_handles() {
        let mut hub = AssetHub::new();

        let (scene_data, events) = hub.ingest_loaded_scene(raw_scene_data()).unwrap();

        assert_eq!(scene_data.meshes.len(), 1);
        assert_eq!(scene_data.materials.len(), 1);
        assert_eq!(scene_data.instances.len(), 1);
        assert_eq!(scene_data.instances[0].mesh, scene_data.meshes[0]);
        assert_eq!(scene_data.instances[0].materials, vec![scene_data.materials[0]]);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], LoadedAssetEvent::MeshLoaded { .. }));
    }

    #[test]
    fn load_scene_deduplicates_by_path() {
        let mut hub = AssetHub::new();
        let path = PathBuf::from("assets/model.fbx");

        let first = hub.load_scene(path.clone());
        let second = hub.load_scene(path.clone());

        assert_eq!(first, second);
        assert_eq!(hub.scene_handle_by_key(&SceneAssetKey { source_path: path }), Some(first));
        assert_eq!(hub.get_scene_status(first), LoadStatus::Loading);
    }
}
