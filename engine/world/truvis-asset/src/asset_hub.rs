use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use slotmap::SlotMap;

use crate::asset_loader::{AssetLoader, LoadResult, SceneLoadRequest, TextureLoadRequest};
use crate::handle::{
    AssetMaterialHandle, AssetMaterialKey, AssetMeshHandle, AssetMeshKey, AssetSceneHandle, AssetSceneKey,
    AssetTextureHandle, LoadStatus, MaterialData, MeshData, RawMaterialData, RawSceneData, SceneData,
    SceneInstanceData, TextureBytes,
};

/// `AssetHub` 内部的 material 记录。
///
/// 这里的 material 是内容资产身份和 CPU 参数，不是渲染后端的稳定 material slot。
pub(crate) struct AssetMaterialRecord {
    pub(crate) status: LoadStatus,
    pub(crate) data: MaterialData,
}

/// `AssetHub` 内部的 scene / prefab 记录。
///
/// `data` 在后台导入完成并完成内部 mesh/material handle 映射后才会填入。
/// runtime instance 由 scene 层根据该 prefab 数据显式 spawn。
pub(crate) struct AssetSceneRecord {
    pub(crate) status: LoadStatus,
    pub(crate) data: Option<SceneData>,
}

/// asset 层向外发布的 CPU ready 事件。
///
/// 渲染后端消费 texture / mesh / material 事件继续做 GPU 上传或 slot 分配；scene
/// 事件表示 prefab CPU 数据已经可被 `SceneManager` 查询并 spawn。失败事件只描述
/// CPU 加载或导入失败。
#[derive(Debug)]
pub enum AssetLoadedEvent {
    /// 纹理文件已经完成 CPU 解码。
    ///
    /// 事件携带一次性的 upload-ready bytes，预期由 render backend 的
    /// `AssetTextureUploader` 消费并创建 GPU image / view / bindless binding。
    /// `AssetHub` 不保留这份像素数据。
    TextureLoaded {
        handle: AssetTextureHandle,
        data: TextureBytes,
    },
    /// 纹理 CPU 加载或解码失败。
    ///
    /// 失败只覆盖 asset 层的文件读取和解码阶段；它不描述 GPU 上传失败。
    TextureFailed { handle: AssetTextureHandle, error: String },
    /// mesh CPU 数据已经可用于渲染侧上传。
    ///
    /// 事件通常来自 scene 导入或显式 `register_mesh_data`。消费方需要继续创建
    /// vertex/index buffer，并在需要 ray tracing 时构建 BLAS。
    MeshLoaded { handle: AssetMeshHandle, data: MeshData },
    /// material CPU 参数已经可用于渲染侧 slot 分配。
    ///
    /// 事件通常来自 scene 导入或显式 `register_material_data`。消费方需要继续分配
    /// shader 可见 material slot，并按 texture ready 状态写入 material buffer。
    MaterialLoaded {
        handle: AssetMaterialHandle,
        data: MaterialData,
    },
    /// scene / prefab 的 CPU 数据已经写入 `AssetHub`。
    ///
    /// 这只表示 `get_scene_data` 可以取得 prefab 数据；live runtime instance 仍需由
    /// `SceneManager` 显式 spawn。
    SceneLoaded { handle: AssetSceneHandle },
    /// scene 文件读取、Assimp 导入或 raw 数据转换失败。
    ///
    /// 失败不代表已经 spawn 的 runtime instance 或 GPU scene 数据发生变化。
    SceneFailed { handle: AssetSceneHandle, error: String },
}

/// 资产中心。
///
/// 这是 world 层访问内容资产的统一入口，负责路径/key 去重、handle 分配、
/// CPU 加载状态和后台任务结果汇聚。它不创建 GPU 资源，也不保存 runtime scene
/// instance；这些职责分别属于 render backend uploader / bridge 和 `SceneManager`。
pub struct AssetHub {
    textures: SlotMap<AssetTextureHandle, LoadStatus>,
    meshes: SlotMap<AssetMeshHandle, LoadStatus>,
    materials: SlotMap<AssetMaterialHandle, AssetMaterialRecord>,
    scenes: SlotMap<AssetSceneHandle, AssetSceneRecord>,

    path_to_texture: HashMap<PathBuf, AssetTextureHandle>,
    key_to_mesh: HashMap<AssetMeshKey, AssetMeshHandle>,
    key_to_material: HashMap<AssetMaterialKey, AssetMaterialHandle>,
    key_to_scene: HashMap<AssetSceneKey, AssetSceneHandle>,

    pending_events: VecDeque<AssetLoadedEvent>,

    loader: AssetLoader,
}

impl Default for AssetHub {
    fn default() -> Self {
        Self::new()
    }
}

// 创建与初始化
impl AssetHub {
    /// 创建空的资产中心。
    ///
    /// 新实例没有任何内容 handle，也没有未消费事件。后台 loader 会随 hub 一起创建，
    /// 后续所有异步结果都必须通过 `update()` 回到调用线程。
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
    /// 消耗资产中心。
    ///
    /// 当前 asset 层没有额外显式释放逻辑；真正需要等待的是内部 `AssetLoader` 的
    /// `Drop`，它会在 hub 被消费后等待后台任务结束。
    pub fn destroy(self) {}
}

// 对外接口
impl AssetHub {
    /// 请求加载纹理。
    ///
    /// 同一路径只分配一个稳定的 `AssetTextureHandle`。如果 handle 已存在，本函数只返回
    /// 已有 handle，不会重复排队后台任务。
    pub fn load_texture(&mut self, path: PathBuf) -> AssetTextureHandle {
        let _span = tracy_client::span!("AssetHub::load_texture");
        if let Some(&handle) = self.path_to_texture.get(&path) {
            return handle;
        }

        let handle = self.textures.insert(LoadStatus::Loading);
        self.path_to_texture.insert(path.clone(), handle);

        log::info!("Request load texture: {:?}", path);
        self.loader.request_load_texture(TextureLoadRequest { path, handle });

        handle
    }

    /// 请求后台导入 scene / prefab。
    ///
    /// 返回的 handle 只代表 CPU scene asset。导入完成后，`update()` 会把 raw
    /// mesh/material/instance index 转换为稳定 asset handle，并发出 `SceneLoaded`；
    /// runtime instance 需要在 scene ready 后由 `SceneManager` 显式 spawn。
    pub fn load_scene(&mut self, path: PathBuf) -> AssetSceneHandle {
        let _span = tracy_client::span!("AssetHub::load_scene");
        let key = AssetSceneKey {
            source_path: path.clone(),
        };
        if let Some(&handle) = self.key_to_scene.get(&key) {
            return handle;
        }

        let handle = self.scenes.insert(AssetSceneRecord {
            status: LoadStatus::Loading,
            data: None,
        });
        self.key_to_scene.insert(key, handle);

        log::info!("Request load scene: {:?}", path);
        self.loader.request_load_scene(SceneLoadRequest { path, handle });

        handle
    }

    /// 注册已经位于 CPU 内存中的 mesh 数据。
    ///
    /// 这通常用于导入器已经复制完 owned mesh 数据的场景。同一个 key 只会产出一次
    /// `MeshLoaded` 事件；事件被渲染后端消费后才会进入 GPU 上传和 BLAS 构建流程。
    pub fn register_mesh_data(&mut self, key: AssetMeshKey, data: MeshData) -> AssetMeshHandle {
        let _span = tracy_client::span!("AssetHub::register_mesh_data");
        let (handle, event) = self.register_mesh_data_inner(key, data);
        if let Some(event) = event {
            self.pending_events.push_back(event);
        }
        handle
    }

    /// 注册已经位于 CPU 内存中的 material 数据。
    ///
    /// GPU material slot 由 render-side `MaterialBridge` 分配，`AssetHub` 只保存内容身份、
    /// 参数和 texture handle 引用。
    pub fn register_material_data(&mut self, key: AssetMaterialKey, data: MaterialData) -> AssetMaterialHandle {
        let _span = tracy_client::span!("AssetHub::register_material_data");
        let (handle, event) = self.register_material_data_inner(key, data);
        if let Some(event) = event {
            self.pending_events.push_back(event);
        }
        handle
    }

    /// 收集后台加载任务完成事件。
    ///
    /// 该函数是后台 loader 和外部渲染/scene 系统之间的同步点：它先排出同步注册产生的
    /// pending events，再把异步结果写回 `AssetHub` 状态表，并返回需要后续系统消费的
    /// CPU ready / failed 事件。
    ///
    /// 调用方通常每帧调用一次，并按事件类型分发给 texture uploader、mesh uploader、
    /// material bridge 或 scene 层。返回后的事件队列已经被消费，`AssetHub` 不会再次
    /// 重放同一事件。
    pub fn update(&mut self) -> Vec<AssetLoadedEvent> {
        let _span = tracy_client::span!("AssetHub::update");
        let mut events = Vec::new();

        while let Some(event) = self.pending_events.pop_front() {
            events.push(event);
        }

        while let Some(result) = self.loader.try_recv_result() {
            match result {
                LoadResult::TextureSuccess { handle, data } => {
                    if let Some(status) = self.textures.get_mut(handle) {
                        *status = LoadStatus::Ready;
                    }

                    events.push(AssetLoadedEvent::TextureLoaded { handle, data });
                }
                LoadResult::TextureFailure(handle, error) => {
                    if let Some(status) = self.textures.get_mut(handle) {
                        *status = LoadStatus::Failed;
                    }

                    events.push(AssetLoadedEvent::TextureFailed { handle, error });
                }
                LoadResult::SceneSuccess { handle, data } => match self.register_loaded_scene(data) {
                    Ok((scene_data, mut scene_events)) => {
                        if let Some(record) = self.scenes.get_mut(handle) {
                            record.status = LoadStatus::Ready;
                            record.data = Some(scene_data);
                        }
                        events.append(&mut scene_events);
                        events.push(AssetLoadedEvent::SceneLoaded { handle });
                    }
                    Err(error) => {
                        if let Some(record) = self.scenes.get_mut(handle) {
                            record.status = LoadStatus::Failed;
                        }
                        events.push(AssetLoadedEvent::SceneFailed { handle, error });
                    }
                },
                LoadResult::SceneFailure(handle, error) => {
                    if let Some(record) = self.scenes.get_mut(handle) {
                        record.status = LoadStatus::Failed;
                    }

                    events.push(AssetLoadedEvent::SceneFailed { handle, error });
                }
            }
        }

        events
    }
}

// 访问与查询
impl AssetHub {
    /// 查询纹理 CPU 加载状态。
    ///
    /// 无效或已不属于当前 `AssetHub` 的 handle 会返回 `Failed`。调用方如果需要区分
    /// “加载失败”和“handle 不存在”，应先通过路径/key 查询确认 handle 身份。
    pub fn get_status(&self, handle: AssetTextureHandle) -> LoadStatus {
        self.textures.get(handle).copied().unwrap_or(LoadStatus::Failed)
    }

    /// 查询 mesh CPU 加载状态。
    ///
    /// 无效 handle 返回 `Failed`；`Ready` 只表示 CPU mesh data 已经注册并发出过上传事件。
    pub fn get_mesh_status(&self, handle: AssetMeshHandle) -> LoadStatus {
        self.meshes.get(handle).copied().unwrap_or(LoadStatus::Failed)
    }

    /// 查询 material CPU 加载状态。
    ///
    /// 无效 handle 返回 `Failed`；`Ready` 不表示 GPU material slot 或引用 texture 已就绪。
    pub fn get_material_status(&self, handle: AssetMaterialHandle) -> LoadStatus {
        self.materials.get(handle).map(|record| record.status).unwrap_or(LoadStatus::Failed)
    }

    /// 查询 scene / prefab CPU 加载状态。
    ///
    /// 无效 handle 返回 `Failed`；`Ready` 只表示 prefab 数据可被查询并用于 spawn。
    pub fn get_scene_status(&self, handle: AssetSceneHandle) -> LoadStatus {
        self.scenes.get(handle).map(|record| record.status).unwrap_or(LoadStatus::Failed)
    }

    /// 按内容路径查询纹理 handle。
    ///
    /// 查询使用 `PathBuf` 的精确词法匹配，不做 canonicalize、symlink 解析或文件访问。
    pub fn texture_handle_by_path(&self, path: &Path) -> Option<AssetTextureHandle> {
        self.path_to_texture.get(path).copied()
    }

    /// 按 mesh key 查询已分配 handle。
    ///
    /// 只有先前通过 scene 导入或 `register_mesh_data` 注册过的 key 才会命中。
    pub fn mesh_handle_by_key(&self, key: &AssetMeshKey) -> Option<AssetMeshHandle> {
        self.key_to_mesh.get(key).copied()
    }

    /// 按 material key 查询已分配 handle。
    ///
    /// 该查询不触发加载，也不检查对应 texture handle 的 GPU ready 状态。
    pub fn material_handle_by_key(&self, key: &AssetMaterialKey) -> Option<AssetMaterialHandle> {
        self.key_to_material.get(key).copied()
    }

    /// 按 scene key 查询已分配 handle。
    ///
    /// 查询只反映 `AssetHub` 的去重表；返回 handle 后仍需通过 `get_scene_status`
    /// 或 `get_scene_data` 判断 CPU 数据是否可用。
    pub fn scene_handle_by_key(&self, key: &AssetSceneKey) -> Option<AssetSceneHandle> {
        self.key_to_scene.get(key).copied()
    }

    /// 获取 CPU material 数据。
    ///
    /// 返回 `None` 表示 handle 无效或 material 尚未注册。返回的数据仍可能引用尚未
    /// CPU/GPU ready 的 texture handle。
    pub fn get_material_data(&self, handle: AssetMaterialHandle) -> Option<&MaterialData> {
        self.materials.get(handle).map(|record| &record.data)
    }

    /// 获取 scene / prefab CPU 数据。
    ///
    /// 只有 scene 导入并经过 `update()` 写回后才会返回 `Some`。调用方不应把返回数据
    /// 当作 live scene；需要交给 `SceneManager` spawn。
    pub fn get_scene_data(&self, handle: AssetSceneHandle) -> Option<&SceneData> {
        self.scenes.get(handle).and_then(|record| record.data.as_ref())
    }

    /// 遍历所有已注册的 CPU material 数据。
    ///
    /// render-side `MaterialBridge` 通过该视图把 asset material 同步为 GPU material
    /// slot；遍历本身不产生事件，也不推进 texture 上传。
    pub fn iter_materials(&self) -> impl Iterator<Item = (AssetMaterialHandle, &MaterialData)> + '_ {
        self.materials.iter().map(|(handle, record)| (handle, &record.data))
    }
}

// 实现细节
impl AssetHub {
    /// 内部 mesh 注册路径，供同步注册和 scene ingest 复用。
    ///
    /// 返回的事件只在第一次看到 key 时产生，避免重复上传同一份 mesh 内容。
    fn register_mesh_data_inner(
        &mut self,
        key: AssetMeshKey,
        data: MeshData,
    ) -> (AssetMeshHandle, Option<AssetLoadedEvent>) {
        if let Some(&handle) = self.key_to_mesh.get(&key) {
            return (handle, None);
        }

        let handle = self.meshes.insert(LoadStatus::Ready);
        self.key_to_mesh.insert(key, handle);
        (handle, Some(AssetLoadedEvent::MeshLoaded { handle, data }))
    }

    /// 内部 material 注册路径，供同步注册和 scene ingest 复用。
    ///
    /// 返回的事件只在第一次看到 key 时产生，避免重复分配同一份 material GPU slot。
    fn register_material_data_inner(
        &mut self,
        key: AssetMaterialKey,
        data: MaterialData,
    ) -> (AssetMaterialHandle, Option<AssetLoadedEvent>) {
        if let Some(&handle) = self.key_to_material.get(&key) {
            return (handle, None);
        }

        let event_data = data.clone();
        let handle = self.materials.insert(AssetMaterialRecord {
            status: LoadStatus::Ready,
            data,
        });
        self.key_to_material.insert(key, handle);
        (
            handle,
            Some(AssetLoadedEvent::MaterialLoaded {
                handle,
                data: event_data,
            }),
        )
    }

    /// 将后台导入器返回的 raw material 转为 asset 层 material 数据。
    ///
    /// texture 路径在这里按 scene 源路径解析，并通过 `load_texture` 进入统一的路径去重
    /// 和异步纹理加载流程。
    fn material_data_from_raw(&mut self, source_path: &Path, raw: RawMaterialData) -> MaterialData {
        MaterialData {
            base_color: raw.base_color,
            emissive: raw.emissive,
            metallic: raw.metallic,
            roughness: raw.roughness,
            opaque: raw.opaque,
            diffuse_texture: raw
                .diffuse_texture_path
                .map(|path| self.load_texture(helper::resolve_scene_texture_path(source_path, path))),
            normal_texture: raw
                .normal_texture_path
                .map(|path| self.load_texture(helper::resolve_scene_texture_path(source_path, path))),
            name: raw.name,
        }
    }

    /// 吸收一次 scene 导入结果。
    ///
    /// 这里是 raw importer index 和稳定 asset handle 的转换点：mesh/material 会先注册
    /// 到 hub，instance 内部引用再从 index 映射到 handle。任何越界引用都会转换为
    /// `SceneFailed` 事件，而不是留下半初始化的 scene data。
    fn register_loaded_scene(&mut self, raw: RawSceneData) -> Result<(SceneData, Vec<AssetLoadedEvent>), String> {
        let source_path = raw.source_path;
        let mut immediate_events = Vec::new();

        let mut mesh_handles = Vec::with_capacity(raw.meshes.len());
        for (mesh_index, mesh_data) in raw.meshes.into_iter().enumerate() {
            let (handle, event) = self.register_mesh_data_inner(
                AssetMeshKey {
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
            let data = self.material_data_from_raw(&source_path, material_data);
            let (handle, event) = self.register_material_data_inner(
                AssetMaterialKey {
                    source_path: source_path.clone(),
                    material_index: material_index as u32,
                },
                data,
            );
            material_handles.push(handle);
            if let Some(event) = event {
                immediate_events.push(event);
            }
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

            instances.push(SceneInstanceData {
                mesh,
                materials,
                transform: instance.transform,
                name: instance.name,
            });
        }

        Ok((
            SceneData {
                source_path,
                name: raw.name,
                meshes: mesh_handles,
                materials: material_handles,
                instances,
            },
            immediate_events,
        ))
    }
}

mod helper {
    use std::path::{Component, Path, PathBuf};

    /// 将 scene 内引用的 texture path 解析为 `AssetHub` 使用的内容路径。
    ///
    /// Assimp 通常返回模型文件内的相对路径；这里只做词法归一化，不访问文件系统，
    /// 让纹理暂缺时仍沿用 `load_texture` 的失败路径。
    pub(super) fn resolve_scene_texture_path(source_path: &Path, texture_path: PathBuf) -> PathBuf {
        let path = if texture_path.is_absolute() {
            texture_path
        } else {
            source_path.parent().unwrap_or_else(|| Path::new("")).join(texture_path)
        };

        normalize_path_lexically(path)
    }

    pub(super) fn normalize_path_lexically(path: PathBuf) -> PathBuf {
        let mut normalized = PathBuf::new();

        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    if !normalized.pop() {
                        normalized.push(component.as_os_str());
                    }
                }
                Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                    normalized.push(component.as_os_str());
                }
            }
        }

        normalized
    }
}
