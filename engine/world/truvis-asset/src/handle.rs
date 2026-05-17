use std::path::PathBuf;

use slotmap::new_key_type;

use ash::vk;

new_key_type! { pub struct AssetTextureHandle; }
new_key_type! { pub struct AssetMeshHandle; }
new_key_type! { pub struct AssetMaterialHandle; }
new_key_type! { pub struct AssetSceneHandle; }

/// 解码后的纹理数据。
///
/// 这里的数据已经是上传友好的 CPU bytes，但还没有创建任何 GPU 资源。
#[derive(Debug)]
pub struct LoadedTextureBytes {
    pub pixels: Vec<u8>,
    pub extent: vk::Extent3D,
    pub format: vk::Format,
}

/// 一个导入源内的 mesh 资产身份。
///
/// `AssetHub` 使用它做路径 + mesh index 去重；它不代表运行时 instance。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MeshAssetKey {
    pub source_path: PathBuf,
    pub mesh_index: u32,
}

/// 一个导入源内的 material 资产身份。
///
/// `AssetHub` 使用它做路径 + material index 去重；它不代表 GPU material slot。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MaterialAssetKey {
    pub source_path: PathBuf,
    pub material_index: u32,
}

/// 一个导入源对应的 scene / prefab 资产身份。
///
/// `AssetSceneHandle` 只代表导入结果，不代表已经 spawn 到运行时场景中的 instance。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SceneAssetKey {
    pub source_path: PathBuf,
}

/// upload-ready 的 CPU mesh 数据。
///
/// 数据已经从导入库的临时内存复制到 Rust owned buffer，但还没有创建 GPU buffer 或 BLAS。
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
/// 这里保存的是内容材质身份关联的参数和 texture handle，不包含 GPU material slot。
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
/// mesh/material 引用已经转换为 `Asset*Handle`，但这里不拥有 live `InstanceHandle`。
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSceneInstanceData {
    pub mesh: AssetMeshHandle,
    pub materials: Vec<AssetMaterialHandle>,
    pub transform: glam::Mat4,
    pub name: String,
}

/// 导入后的 scene / prefab CPU 数据。
///
/// 它保存导入结果内部引用关系，可被多次 spawn 到 `SceneManager`。
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
/// texture 仍以路径表达，交回 `AssetHub::update()` 统一分配 texture handle。
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
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawLoadedSceneInstanceData {
    pub mesh_index: u32,
    pub material_indices: Vec<u32>,
    pub transform: glam::Mat4,
    pub name: String,
}

/// 后台 Assimp task 产出的 owned scene CPU 数据。
///
/// 这里不保存任何 C++ handle 或 raw pointer，`truvixx_scene_free` 已经在 task 内完成。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawLoadedSceneData {
    pub source_path: PathBuf,
    pub name: String,
    pub meshes: Vec<LoadedMeshData>,
    pub materials: Vec<RawLoadedMaterialData>,
    pub instances: Vec<RawLoadedSceneInstanceData>,
}

/// 资源加载状态机
///
/// 对 `AssetHub` 而言，`Ready` 只表示 CPU 侧数据已经可用，不表示 GPU 可用。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LoadStatus {
    /// 初始状态，资源尚未请求加载
    Unloaded,
    /// IO 阶段：正在后台线程读取文件或进行 CPU 解码 (如 png -> rgba8)
    Loading,
    /// 完成状态：文件已经加载并解码为 CPU bytes
    Ready,
    /// 失败状态：文件不存在、格式错误或解码失败
    Failed,
}
