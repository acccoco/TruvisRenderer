use std::path::PathBuf;

use slotmap::new_key_type;

use ash::vk;

new_key_type! {
    /// 纹理内容资产身份。
    ///
    /// 该 handle 由 `AssetHub` 按路径去重后分配，只用于跨 world / render
    /// 边界引用同一份纹理内容。它不表示 Vulkan image、image view、bindless
    /// descriptor 或 shader 可见 binding 已经存在。
    pub struct AssetTextureHandle;
}

new_key_type! {
    /// mesh 内容资产身份。
    ///
    /// 该 handle 由 `AssetHub` 按 key 去重后分配。mesh CPU 数据只通过
    /// `AssetLoadedEvent::MeshLoaded` 一次性交给渲染侧上传器，asset 层只维护
    /// 内容身份和 CPU 加载状态，不保存可查询的 mesh 数据副本。
    pub struct AssetMeshHandle;
}

new_key_type! {
    /// material 内容资产身份。
    ///
    /// 该 handle 指向 CPU 侧材质参数和 texture asset 引用。它不是
    /// `MaterialManager` 的稳定 GPU material slot，也不承诺引用的 texture 已经上传。
    pub struct AssetMaterialHandle;
}

new_key_type! {
    /// model / prefab 内容资产身份。
    ///
    /// 该 handle 指向后台导入得到的可重复 spawn 的模型 CPU 数据，不是
    /// `SceneManager` 中的 live runtime instance handle。
    pub struct AssetModelHandle;
}

/// 一个导入源内的 mesh 内容身份。
///
/// `AssetHub` 使用 `source_path + mesh_index` 做去重，保证同一 model / prefab
/// 导入结果内的 mesh 只对应一个稳定 asset handle。它不代表运行时 instance，
/// 也不代表渲染后端已经创建的 vertex/index buffer 或 BLAS。
///
/// `source_path` 使用调用方传入或导入结果保留的路径表达；这里不做文件系统
/// canonicalize，因此调用方需要在入口处维持一致的路径策略。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetMeshKey {
    pub source_path: PathBuf,
    pub mesh_index: u32,
}

/// 一个导入源内的 material 内容身份。
///
/// `AssetHub` 使用 `source_path + material_index` 做去重。对应 handle 表示 CPU
/// material 参数和 texture 引用身份，不表示 render-side `MaterialManager`
/// 分配出的 GPU material slot。
///
/// 与 mesh key 一样，`source_path` 是词法路径身份的一部分；不同路径写法会被视为
/// 不同导入源。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetMaterialKey {
    pub source_path: PathBuf,
    pub material_index: u32,
}

/// 一个导入源对应的 model / prefab 内容身份。
///
/// `AssetModelHandle` 只代表后台导入得到的模型 CPU 数据。它可以被
/// `SceneManager` 多次 spawn 成运行时 instance，但自身不持有 live scene 状态。
///
/// `source_path` 是 model 去重 key 的完整内容，`AssetHub::load_model` 不会在这里
/// 解析 symlink 或访问文件系统做 canonicalize。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetModelKey {
    pub source_path: PathBuf,
}

/// 解码后的纹理 CPU 数据。
///
/// 这是 asset 层传给渲染后端 uploader 的边界格式：像素已经位于 owned
/// CPU buffer，并带有 Vulkan 上传所需的 extent / format 元数据，但还没有创建
/// image、image view 或 bindless descriptor。
///
/// 与 mesh / material / model 不同，当前纹理 bytes 只通过
/// `AssetLoadedEvent::TextureLoaded` 交给 uploader，`AssetHub` 本身只保存路径和
/// CPU 加载状态。
#[derive(Debug)]
pub struct TextureBytes {
    pub pixels: Vec<u8>,
    pub extent: vk::Extent3D,
    pub format: vk::Format,
}

/// upload-ready 的 CPU mesh 数据。
///
/// 数据已经从导入库的临时内存复制到 Rust owned buffer。asset 层在这里停止，
/// 后续的 vertex/index buffer 创建、BLAS 构建和 GPU ready 状态由
/// `AssetMeshUploader` 维护。
///
/// 调用方应保持顶点属性数组长度一致，`indices` 使用 `u32` 索引。asset 层不在
/// 注册时重建或修复几何拓扑。
#[derive(Debug, Clone, PartialEq)]
pub struct MeshData {
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
///
/// 材质本身 `Ready` 只表示这些 CPU 参数已经可读取。引用的 texture 是否存在真实
/// SRV 需要渲染侧通过 texture resolver 再检查。
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialData {
    pub base_color: glam::Vec4,
    pub emissive: glam::Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub opaque: f32,

    pub diffuse_texture: Option<AssetTextureHandle>,
    pub normal_texture: Option<AssetTextureHandle>,
    pub name: String,
}

/// Model asset 内部的一个可 spawn runtime instance 记录。
///
/// mesh/material 引用已经转换为 `Asset*Handle`，用于把 prefab 内部引用关系传给
/// `SceneManager::spawn_model`。这里不拥有 live `InstanceHandle`，也不表达
/// GPU instance slot。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelInstanceData {
    pub mesh: AssetMeshHandle,
    pub materials: Vec<AssetMaterialHandle>,
    pub transform: glam::Mat4,
    pub name: String,
}

/// 导入后的 model / prefab CPU 数据。
///
/// 它保存导入结果的 mesh、material 和 instance 引用关系，是 asset 层交给 scene
/// 层的 prefab 数据。多个 runtime scene instance 可以共享同一个 `ModelData` 中的
/// asset handle。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelData {
    pub source_path: PathBuf,
    pub name: String,
    pub meshes: Vec<AssetMeshHandle>,
    pub materials: Vec<AssetMaterialHandle>,
    pub instances: Vec<ModelInstanceData>,
}

/// 后台 Assimp task 产出的 owned material CPU 数据。
///
/// texture 仍以导入器返回的路径表达，避免后台 task 直接修改 `AssetHub`。
/// `AssetHub::update()` 在 render/world 侧收敛任务结果后统一解析相对路径、
/// 分配 texture handle，并维持路径去重表。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawMaterialData {
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
pub(crate) struct RawSceneInstanceData {
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
pub(crate) struct RawSceneData {
    pub source_path: PathBuf,
    pub name: String,
    pub meshes: Vec<MeshData>,
    pub materials: Vec<RawMaterialData>,
    pub instances: Vec<RawSceneInstanceData>,
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
    /// IO / CPU 阶段：后台线程正在读取文件、解码纹理或导入 model。
    Loading,
    /// CPU 完成状态：数据已经进入 `AssetHub` 可读缓存或已发出完成事件。
    Ready,
    /// 失败状态：文件不存在、格式错误、解码失败或导入器返回错误。
    Failed,
}
