use crate::bindless_manager::BindlessSrvHandle;
use crate::geometry::RtGeometry;
use ash::vk;
use truvis_shader_binding::gpu;

/// 用于渲染的完整实例数据（只读快照）
///
/// 包含从 SceneManager 中提取的所有必要信息，
/// 使得 GpuScene 可以独立于 SceneManager 完成 GPU buffer 的构建和上传。
#[derive(Clone)]
pub struct InstanceRenderData {
    /// 该实例使用的 mesh 在 `SceneData2::all_meshes` 中的索引
    pub mesh_index: usize,
    /// 该实例的每个 submesh 对应的材质索引（在 `SceneData2::all_materials` 中）
    pub material_indices: Vec<usize>,
    /// 实例的变换矩阵
    pub transform: glam::Mat4,
}

/// 用于渲染的完整材质数据（只读快照）
#[derive(Clone, Default)]
pub struct MaterialRenderData {
    pub base_color: glam::Vec4,
    pub emissive: glam::Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub opaque: f32,

    /// 漫反射贴图的 Bindless Handle（如果没有则为 null）
    pub diffuse_bindless_handle: BindlessSrvHandle,
    /// 法线贴图的 Bindless Handle（如果没有则为 null）
    pub normal_bindless_handle: BindlessSrvHandle,
}

/// 用于渲染的完整 Mesh 数据引用（只读快照）
///
/// 注意：由于 RtGeometry 包含 GPU buffer，这里使用引用而非拷贝
pub struct MeshRenderData<'a> {
    /// 该 mesh 包含的所有几何体数据
    pub geometries: &'a [RtGeometry],
    /// BLAS 的设备地址（用于 TLAS 构建）
    pub blas_device_address: Option<vk::DeviceAddress>,
    /// Mesh 名称
    pub name: &'a str,
}

/// 由 SceneManager 构建的完整场景数据快照（只读）
///
/// 这是一个自包含的场景数据结构，GpuScene 可以仅凭此结构完成
/// GPU buffer 的构建和上传，无需访问 SceneManager。
///
/// # 设计原则
/// - 所有数据都是只读的，由 SceneManager 负责构建
/// - 使用索引引用而非 Handle，简化 GPU 端数据查找
/// - 保持数据顺序一致性，确保索引有效
///
/// # 生命周期
/// 由于 `all_meshes` 持有对 Mesh geometries 的引用，
/// SceneData2 的生命周期受限于 SceneManager 中的 Mesh 数据。
pub struct RenderData<'a> {
    /// 所有实例数据（按顺序）
    pub all_instances: Vec<InstanceRenderData>,
    /// 所有 mesh 数据引用（按顺序）
    pub all_meshes: Vec<MeshRenderData<'a>>,
    /// 所有材质数据（按顺序）
    pub all_materials: Vec<MaterialRenderData>,
    /// 所有点光源数据
    pub all_point_lights: Vec<gpu::PointLight>,

    /// 每个 mesh 在 geometry buffer 中的起始索引（预计算）
    /// 长度与 all_meshes 相同
    pub mesh_geometry_start_indices: Vec<usize>,
    /// 总 geometry 数量（预计算）
    pub total_geometry_count: usize,
}
impl<'a> RenderData<'a> {
    /// 创建一个空的场景数据
    pub fn empty() -> Self {
        Self {
            all_instances: Vec::new(),
            all_meshes: Vec::new(),
            all_materials: Vec::new(),
            all_point_lights: Vec::new(),
            mesh_geometry_start_indices: Vec::new(),
            total_geometry_count: 0,
        }
    }

    /// 检查场景是否为空
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.all_instances.is_empty()
            && self.all_meshes.is_empty()
            && self.all_materials.is_empty()
            && self.all_point_lights.is_empty()
    }

    /// 获取指定 mesh 的 geometry 数据
    #[inline]
    pub fn get_mesh_geometries(&self, mesh_index: usize) -> Option<&[RtGeometry]> {
        self.all_meshes.get(mesh_index).map(|m| m.geometries)
    }

    /// 获取指定 mesh 在 geometry buffer 中的起始索引
    #[inline]
    pub fn get_mesh_geometry_start_index(&self, mesh_index: usize) -> Option<usize> {
        self.mesh_geometry_start_indices.get(mesh_index).copied()
    }
}
impl Default for RenderData<'_> {
    fn default() -> Self {
        Self::empty()
    }
}
