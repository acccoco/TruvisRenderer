use ash::vk;

use truvis_shader_binding::gpu;

use super::geometry::{RtGeometry, RtTriangleMeta};

/// Instance 在 GPU scene instance buffer 中的稳定 slot。
///
/// slot 只保证在当前运行时 instance 生命周期内稳定；销毁后会延迟回收。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct GpuInstanceSlot(u32);

impl GpuInstanceSlot {
    pub(crate) const TLAS_CUSTOM_INDEX_MAX: u32 = 0x00FF_FFFF;

    pub(crate) fn new(index: u32) -> Self {
        Self(index)
    }

    pub(crate) fn as_u32(self) -> u32 {
        self.0
    }

    pub(crate) fn as_usize(self) -> usize {
        self.0 as usize
    }

    pub(crate) fn validate_tlas_custom_index(self) {
        assert!(self.0 <= Self::TLAS_CUSTOM_INDEX_MAX, "TLAS instance custom index exceeds Vulkan 24-bit limit");
    }
}

/// 用于渲染的单个实例快照。
///
/// 它已经把 CPU scene 中的 asset handle 解析为稳定 instance slot、mesh 索引和
/// material slot；`GpuScene` 只消费这些 render-side 索引，不再访问 `SceneManager`。
#[derive(Clone)]
pub(crate) struct InstanceRenderData {
    /// 该实例在 GPU instance buffer 与 TLAS custom index 中共用的稳定 slot。
    pub(crate) instance_slot: GpuInstanceSlot,
    /// 该实例使用的 mesh 在 `RenderData::all_meshes` 中的索引。
    pub(crate) mesh_index: usize,
    /// 该实例每个 submesh 对应的稳定 GPU material slot。
    pub(crate) material_slots: Vec<u32>,
    /// 由 CPU scene 提供的模型矩阵，prepare 阶段会写入 instance buffer 并参与 TLAS 构建。
    pub(crate) transform: glam::Mat4,
    /// 上一帧用于 DLSS motion vector 回溯的模型矩阵。
    ///
    /// 新激活实例或 history reset 帧会由 `InstanceBridge` 写为当前 transform，
    /// 避免旧 slot 历史污染当前帧 temporal 输入。
    pub(crate) previous_transform: glam::Mat4,
}

/// 用于渲染的 mesh GPU 数据引用。
///
/// `RtGeometry` 与 BLAS 生命周期由 `AssetMeshManager` 持有，这里只借用已完成上传的
/// render-side 数据，保证 `RenderData` 构建期间不会复制 GPU 资源 owner。
pub(crate) struct MeshRenderData<'a> {
    /// 该 mesh 包含的所有 submesh 几何体。
    pub(crate) geometries: &'a [RtGeometry],
    /// 与 `geometries` 同顺序的 CPU 三角形元数据，只保存 light table 构建需要的最小字段。
    pub(crate) triangle_metadata: &'a [Vec<RtTriangleMeta>],
    /// BLAS 的设备地址，用于 `GpuScene` 构建 TLAS；未 ready 的 mesh 不应出现在 active 实例中。
    pub(crate) blas_device_address: Option<vk::DeviceAddress>,
}

/// 由 render-side scene bridge 构建的场景数据快照。
///
/// 这是 `InstanceBridge` 交给 `GpuScene` 的 prepare 输入。它只包含依赖已 ready 的实例，
/// 并用稳定 slot 与紧凑索引连接 instance、mesh、geometry、material，避免 GPU 上传阶段
/// 再回到 CPU scene/asset handle 做解析。
///
/// # 设计原则
/// - 所有数据都是只读的，由 render-side bridge 负责构建
/// - 使用索引 / 稳定 slot 引用而非 Handle，简化 GPU 端数据查找
/// - 保持数据顺序一致性，确保索引有效
///
/// # 生命周期
/// 由于 `all_meshes` 持有对 Mesh geometries 的引用，
/// `RenderData` 的生命周期受限于 render-side mesh manager 中的 Mesh 数据。
pub(crate) struct RenderData<'a> {
    /// 所有 active 实例，按稳定 instance slot 排序。
    pub(crate) all_instances: Vec<InstanceRenderData>,
    /// 本帧 active 实例引用到的去重 mesh GPU 数据。
    pub(crate) all_meshes: Vec<MeshRenderData<'a>>,
    /// 当前 CPU scene 中的点光源快照，按 SceneManager 迭代顺序上传。
    pub(crate) all_point_lights: Vec<gpu::light::PointLight>,

    /// 每个 mesh 在 geometry buffer 中的起始索引，长度与 `all_meshes` 相同。
    pub(crate) mesh_geometry_start_indices: Vec<usize>,
}
impl<'a> RenderData<'a> {
    /// 创建一个不包含实例、mesh 或光源的空场景快照。
    pub(crate) fn empty() -> Self {
        Self {
            all_instances: Vec::new(),
            all_meshes: Vec::new(),
            all_point_lights: Vec::new(),
            mesh_geometry_start_indices: Vec::new(),
        }
    }
}
impl Default for RenderData<'_> {
    fn default() -> Self {
        Self::empty()
    }
}
