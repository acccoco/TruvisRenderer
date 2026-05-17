use std::path::PathBuf;

use slotmap::new_key_type;

use ash::vk;

new_key_type! { pub struct AssetTextureHandle; }
new_key_type! { pub struct AssetMeshHandle; }
new_key_type! { pub struct AssetMaterialHandle; }
new_key_type! { pub struct AssetSceneHandle; }

/// 解码后的纹理 CPU 数据。
///
/// 这是 asset 层传给渲染后端 uploader 的边界格式：像素已经位于 owned
/// CPU buffer，并带有 Vulkan 上传所需的 extent / format 元数据，但还没有创建
/// image、image view 或 bindless descriptor。
#[derive(Debug)]
pub struct LoadedTextureBytes {
    pub pixels: Vec<u8>,
    pub extent: vk::Extent3D,
    pub format: vk::Format,
}

/// 一个导入源内的 mesh 内容身份。
///
/// `AssetHub` 使用 `source_path + mesh_index` 做去重，保证同一 scene / prefab
/// 导入结果内的 mesh 只对应一个稳定 asset handle。它不代表运行时 instance，
/// 也不代表渲染后端已经创建的 vertex/index buffer 或 BLAS。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MeshAssetKey {
    pub source_path: PathBuf,
    pub mesh_index: u32,
}

/// 一个导入源内的 material 内容身份。
///
/// `AssetHub` 使用 `source_path + material_index` 做去重。对应 handle 表示 CPU
/// material 参数和 texture 引用身份，不表示 render-side `MaterialManager`
/// 分配出的 GPU material slot。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MaterialAssetKey {
    pub source_path: PathBuf,
    pub material_index: u32,
}

/// 一个导入源对应的 scene / prefab 内容身份。
///
/// `AssetSceneHandle` 只代表后台导入得到的 prefab CPU 数据。它可以被
/// `SceneManager` 多次 spawn 成运行时 instance，但自身不持有 live scene 状态。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SceneAssetKey {
    pub source_path: PathBuf,
}

/// upload-ready 的 CPU mesh 数据。
///
/// 数据已经从导入库的临时内存复制到 Rust owned buffer。asset 层在这里停止，
/// 后续的 vertex/index buffer 创建、BLAS 构建和 GPU ready 状态由
/// `AssetMeshUploader` 维护。
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedMeshData {
    pub positions: Vec<glam::Vec3>,
    pub normals: Vec<glam::Vec3>,
    pub tangents: Vec<glam::Vec3>,
    pub uvs: Vec<glam::Vec2>,
    pub indices: Vec<u32>,
    pub name: String,
}

/// CPU 侧材质资产数据。
///
/// 这里保存的是内容材质身份关联的 PBR 参数和 texture asset handle 引用。
/// texture handle 可能仍在 Loading；GPU material slot、material buffer 写入和
/// texture ready gate 都属于 render-side `MaterialBridge` / `MaterialManager`。
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedMaterialData {
    pub base_color: glam::Vec4,
    pub emissive: glam::Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub opaque: f32,

    pub diffuse_texture: Option<AssetTextureHandle>,
    pub normal_texture: Option<AssetTextureHandle>,
    pub name: String,
}

/// Scene asset 内部的一个可 spawn runtime instance 记录。
///
/// mesh/material 引用已经转换为 `Asset*Handle`，用于把 prefab 内部引用关系传给
/// `SceneManager::spawn_scene_asset`。这里不拥有 live `InstanceHandle`，也不表达
/// GPU instance slot。
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSceneInstanceData {
    pub mesh: AssetMeshHandle,
    pub materials: Vec<AssetMaterialHandle>,
    pub transform: glam::Mat4,
    pub name: String,
}

/// 导入后的 scene / prefab CPU 数据。
///
/// 它保存导入结果的 mesh、material 和 instance 引用关系，是 asset 层交给
/// scene 层的 prefab 数据。多个 runtime scene instance 可以共享同一个
/// `LoadedSceneData` 中的 asset handle。
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSceneData {
    pub source_path: PathBuf,
    pub name: String,
    pub meshes: Vec<AssetMeshHandle>,
    pub materials: Vec<AssetMaterialHandle>,
    pub instances: Vec<LoadedSceneInstanceData>,
}

/// 后台 Assimp task 产出的 owned material CPU 数据。
///
/// texture 仍以导入器返回的路径表达，避免后台 task 直接修改 `AssetHub`。
/// `AssetHub::update()` 在 render/world 侧收敛任务结果后统一解析相对路径、
/// 分配 texture handle，并维持路径去重表。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawLoadedMaterialData {
    pub base_color: glam::Vec4,
    pub emissive: glam::Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub opaque: f32,
    pub diffuse_texture_path: Option<PathBuf>,
    pub normal_texture_path: Option<PathBuf>,
    pub name: String,
}

/// 后台 Assimp task 产出的 owned instance CPU 数据。
///
/// 仍使用导入源内的 mesh/material index，稍后由 `AssetHub` 转换成稳定
/// asset handle，避免把半成品 handle 分配逻辑放入 FFI copy 任务。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawLoadedSceneInstanceData {
    pub mesh_index: u32,
    pub material_indices: Vec<u32>,
    pub transform: glam::Mat4,
    pub name: String,
}

/// 后台 Assimp task 产出的 owned scene CPU 数据。
///
/// 这里不保存任何 C++ handle 或 raw pointer。Assimp / C++ scene 的生命周期
/// 被限制在后台 task 内，`truvixx_scene_free` 已经在返回该结构前完成。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawLoadedSceneData {
    pub source_path: PathBuf,
    pub name: String,
    pub meshes: Vec<LoadedMeshData>,
    pub materials: Vec<RawLoadedMaterialData>,
    pub instances: Vec<RawLoadedSceneInstanceData>,
}

/// asset 层的 CPU 加载状态机。
///
/// 对 `AssetHub` 而言，`Ready` 只表示 CPU 侧数据已经可用。纹理是否已经注册
/// bindless、mesh 是否已有 GPU buffer / BLAS、material 是否已有 GPU slot，
/// 都由渲染后端自己的 manager 再维护一层 ready 状态。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LoadStatus {
    /// 初始状态，资源尚未请求加载。
    Unloaded,
    /// IO / CPU 阶段：后台线程正在读取文件、解码纹理或导入 scene。
    Loading,
    /// CPU 完成状态：数据已经进入 `AssetHub` 可读缓存或已发出完成事件。
    Ready,
    /// 失败状态：文件不存在、格式错误、解码失败或导入器返回错误。
    Failed,
}
