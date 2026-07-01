use std::path::PathBuf;

use slotmap::new_key_type;

use ash::vk;

new_key_type! {
    /// 纹理加载任务身份。
    ///
    /// 该 handle 只用于把后台 texture load result 关联回 `SceneAssetIngestor`，
    /// 不表示长期 texture identity、Vulkan image、image view、bindless descriptor
    /// 或 shader 可见 binding。
    pub struct TextureLoadHandle;
}

new_key_type! {
    /// model / prefab 加载任务身份。
    ///
    /// 该 handle 只用于把后台 model import result 关联回 `SceneAssetIngestor`，
    /// 不是长期 model database key，也不是 `SceneStore` 中的 live runtime instance handle。
    pub struct ModelLoadHandle;
}

/// 一次 texture CPU decode task 的输入描述。
///
/// 这是一次性 loader 请求的参数，不是长期 identity key。同一路径是否复用为同一个
/// `TextureHandle` 由 `SceneAssetIngestor` / `SceneStore` 决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureLoadDesc {
    pub path: std::path::PathBuf,
}

/// 一次 model / prefab CPU import task 的输入描述。
///
/// 这是一次性 loader 请求的参数，不表示长期 model database key，也不参与 scene 去重。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelLoadDesc {
    pub path: std::path::PathBuf,
}

/// 解码后的纹理 CPU 数据。
///
/// 这是 asset 层传给渲染运行时 texture manager 的边界格式：像素已经位于 owned
/// CPU buffer，并带有 Vulkan 上传所需的 extent / format 元数据，但还没有创建
/// image、image view 或 bindless descriptor。
///
/// 当前纹理 bytes 只通过 `AssetLoadEvent::TextureLoaded` 短期交给 `SceneAssetIngestor`
/// 和 render-side texture manager，`AssetHub` 本身不保存像素数据。
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
/// `RenderMeshManager` 维护。
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

/// 后台 Assimp task 产出的 owned material CPU 数据。
///
/// texture 仍以导入器返回的路径表达，避免后台 task 直接修改 `SceneStore`。
/// `SceneAssetIngestor` 在 asset sync 阶段解析相对路径、分配 `TextureHandle`
/// 并提交必要的 texture load task。
#[derive(Debug, Clone, PartialEq)]
pub struct RawMaterialData {
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
/// 仍使用导入源内的 mesh/material index，稍后由 `SceneAssetIngestor` 转换成稳定
/// scene handle，避免把半成品 handle 分配逻辑放入 FFI copy 任务。
#[derive(Debug, Clone, PartialEq)]
pub struct RawSceneInstanceData {
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
pub struct RawSceneData {
    pub source_path: PathBuf,
    pub name: String,
    pub meshes: Vec<MeshData>,
    pub materials: Vec<RawMaterialData>,
    pub instances: Vec<RawSceneInstanceData>,
}

/// asset 层的 CPU 加载状态机。
///
/// 对 loader 而言，`Ready` 只表示 CPU 侧数据已经通过 event 交付给上层。
/// 纹理是否已经注册 bindless、mesh 是否已有 GPU buffer / BLAS、material 是否已有 GPU slot，
/// 都由渲染运行时自己的 manager 再维护一层 ready 状态。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LoadStatus {
    /// 初始状态，资源尚未请求加载。
    Unloaded,
    /// IO / CPU 阶段：后台线程正在读取文件、解码纹理或导入 model。
    Loading,
    /// CPU 完成状态：数据已经通过完成事件交付。
    Ready,
    /// 失败状态：文件不存在、格式错误、解码失败或导入器返回错误。
    Failed,
}
