use ash::vk;

use truvis_shader_binding::gpu;

use super::geometry::RtGeometry;

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

/// 用于渲染的完整实例数据（只读快照）
///
/// 包含从 SceneManager 中提取的所有必要信息，
/// 使得 GpuScene 可以独立于 SceneManager 完成 GPU buffer 的构建和上传。
#[derive(Clone)]
pub(crate) struct InstanceRenderData {
    /// 该实例在 GPU instance buffer 中的稳定 slot
    pub(crate) instance_slot: GpuInstanceSlot,
    /// 该实例使用的 mesh 在 `SceneData2::all_meshes` 中的索引
    pub(crate) mesh_index: usize,
    /// 该实例的每个 submesh 对应的稳定 GPU material slot
    pub(crate) material_slots: Vec<u32>,
    /// 实例的变换矩阵
    pub(crate) transform: glam::Mat4,
}

/// 用于渲染的完整 Mesh 数据引用（只读快照）
///
/// 注意：由于 RtGeometry 包含 GPU buffer，这里使用引用而非拷贝
pub(crate) struct MeshRenderData<'a> {
    /// 该 mesh 包含的所有几何体数据
    pub(crate) geometries: &'a [RtGeometry],
    /// BLAS 的设备地址（用于 TLAS 构建）
    pub(crate) blas_device_address: Option<vk::DeviceAddress>,
}

/// 由 render-side scene bridge 构建的完整场景数据快照（只读）
///
/// 这是一个自包含的场景数据结构，GpuScene 可以仅凭此结构完成
/// GPU buffer 的构建和上传，无需访问 SceneManager。
///
/// # 设计原则
/// - 所有数据都是只读的，由 render-side bridge 负责构建
/// - 使用索引 / 稳定 slot 引用而非 Handle，简化 GPU 端数据查找
/// - 保持数据顺序一致性，确保索引有效
///
/// # 生命周期
/// 由于 `all_meshes` 持有对 Mesh geometries 的引用，
/// SceneData2 的生命周期受限于 render-side mesh uploader 中的 Mesh 数据。
pub(crate) struct RenderData<'a> {
    /// 所有实例数据（按顺序）
    pub(crate) all_instances: Vec<InstanceRenderData>,
    /// 所有 mesh 数据引用（按顺序）
    pub(crate) all_meshes: Vec<MeshRenderData<'a>>,
    /// 所有点光源数据
    pub(crate) all_point_lights: Vec<gpu::PointLight>,

    /// 每个 mesh 在 geometry buffer 中的起始索引（预计算）
    /// 长度与 all_meshes 相同
    pub(crate) mesh_geometry_start_indices: Vec<usize>,
}
impl<'a> RenderData<'a> {
    /// 创建一个空的场景数据
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_instance_slot_keeps_raw_index() {
        let slot = GpuInstanceSlot::new(42);

        assert_eq!(slot.as_u32(), 42);
        assert_eq!(slot.as_usize(), 42);
    }

    #[test]
    fn gpu_instance_slot_accepts_vulkan_24_bit_custom_index_limit() {
        GpuInstanceSlot::new(GpuInstanceSlot::TLAS_CUSTOM_INDEX_MAX).validate_tlas_custom_index();
    }

    #[test]
    #[should_panic(expected = "TLAS instance custom index exceeds Vulkan 24-bit limit")]
    fn gpu_instance_slot_rejects_custom_index_over_vulkan_limit() {
        GpuInstanceSlot::new(GpuInstanceSlot::TLAS_CUSTOM_INDEX_MAX + 1).validate_tlas_custom_index();
    }
}
