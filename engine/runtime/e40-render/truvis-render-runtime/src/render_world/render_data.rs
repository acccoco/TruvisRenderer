use ash::vk;

use truvis_world::guid_new_type::MaterialAssetHandle;

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
/// 它已经把 CPU scene 中的 resource handle 解析为稳定 instance slot、mesh 索引和
/// material slot；`RenderWorld` 只消费这些 render-side 索引，不再访问 `SceneStore`。
#[derive(Clone)]
pub(crate) struct InstanceRenderData {
    /// 该实例在 GPU instance buffer 与 TLAS custom index 中共用的稳定 slot。
    pub(crate) instance_slot: GpuInstanceSlot,
    /// 该实例使用的 mesh 在 `RenderData::all_meshes` 中的索引。
    pub(crate) mesh_index: usize,
    /// 该实例每个 submesh 对应的稳定 GPU material slot。
    pub(crate) material_slots: Vec<u32>,
    /// 该实例每个 submesh 对应的 CPU `MaterialAssetHandle`，与 `material_slots` 顺序一致。
    pub(crate) material_handles: Vec<MaterialAssetHandle>,
    /// true 表示任一 submesh material 需要 any-hit alpha test，TLAS 不能对该实例设置 FORCE_OPAQUE。
    pub(crate) requires_any_hit: bool,
    /// 由 CPU scene 提供的模型矩阵，prepare 阶段会写入 instance buffer 并参与 TLAS 构建。
    pub(crate) transform: glam::Mat4,
}

/// 用于渲染的 mesh GPU 数据引用。
///
/// `RtGeometry` 与 BLAS 生命周期由 `GpuMeshStore` 持有，这里只借用已完成上传的
/// render-side 数据，保证 `RenderData` 构建期间不会复制 GPU 资源 owner。
pub(crate) struct MeshRenderData<'a> {
    /// 该 mesh 包含的所有 submesh 几何体。
    pub(crate) geometries: &'a [RtGeometry],
    /// 与 `geometries` 同顺序的 CPU 三角形元数据，只保存 light table 构建需要的最小字段。
    pub(crate) triangle_metadata: &'a [Vec<RtTriangleMeta>],
    /// BLAS 的设备地址，用于 `RenderWorld` 构建 TLAS；未 ready 的 mesh 不应出现在 active 实例中。
    pub(crate) blas_device_address: Option<vk::DeviceAddress>,
}

/// 由 render-side scene bridge 构建的场景数据快照。
///
/// 这是 `RenderInstanceTable` 交给 `RenderWorld` 的 prepare 输入。它只包含依赖已 ready 的实例，
/// 并用稳定 slot 与紧凑索引连接 instance、mesh、geometry、material，避免 GPU 上传阶段
/// 再回到 CPU resource handle 做解析。
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
    /// 每个 mesh 在 geometry buffer 中的起始索引，长度与 `all_meshes` 相同。
    pub(crate) mesh_geometry_start_indices: Vec<usize>,
}
impl<'a> RenderData<'a> {
    /// 验证紧凑表与稳定 slot 的 GPU 索引契约，同时计算 geometry/instance/indirect 容量。
    /// end 是宿主侧排他上界，允许最后一个有效索引等于 u32::MAX，不把 end 截断写给 shader。
    pub(crate) fn buffer_counts(&self) -> (usize, usize, usize) {
        assert_eq!(
            self.mesh_geometry_start_indices.len(),
            self.all_meshes.len(),
            "mesh geometry offsets are incomplete"
        );
        let mut geometry_count = 0;
        for (mesh, &start) in self.all_meshes.iter().zip(&self.mesh_geometry_start_indices) {
            assert_eq!(start, geometry_count, "mesh geometry offsets must form a contiguous prefix");
            geometry_count = Self::checked_index_end(start, mesh.geometries.len());
        }
        let mut instance_count = 0;
        let mut indirect_count = 0;
        let mut previous_slot = None;
        for instance in &self.all_instances {
            instance.instance_slot.validate_tlas_custom_index();
            let slot = instance.instance_slot.as_usize();
            assert!(previous_slot.is_none_or(|previous| previous < slot), "instance slots must be unique and sorted");
            previous_slot = Some(slot);
            instance_count = slot.checked_add(1).expect("instance capacity overflow");
            let mesh = self.all_meshes.get(instance.mesh_index).expect("instance mesh index is invalid");
            assert_eq!(instance.material_slots.len(), mesh.geometries.len(), "instance materials must match mesh geometries");
            assert_eq!(
                instance.material_handles.len(),
                instance.material_slots.len(),
                "instance material identities are incomplete"
            );
            u32::try_from(indirect_count).expect("instance indirect offset exceeds u32");
            indirect_count = Self::checked_index_end(indirect_count, instance.material_slots.len());
        }
        (geometry_count, instance_count, indirect_count)
    }

    /// count 与每个实际索引都必须能写入 GPU uint；空范围不产生索引。
    fn checked_index_end(start: usize, count: usize) -> usize {
        u32::try_from(count).expect("scene submesh count exceeds u32");
        let end = start.checked_add(count).expect("scene index range overflow");
        if count != 0 {
            u32::try_from(end - 1).expect("scene index exceeds u32");
        }
        end
    }

    /// 创建一个不包含实例、mesh 或光源的空场景快照。
    pub(crate) fn empty() -> Self {
        Self {
            all_instances: Vec::new(),
            all_meshes: Vec::new(),
            mesh_geometry_start_indices: Vec::new(),
        }
    }
}
impl Default for RenderData<'_> {
    fn default() -> Self {
        Self::empty()
    }
}
