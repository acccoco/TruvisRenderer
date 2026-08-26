use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::protocol::{InstanceId, MaterialId, MeshId, SceneVersion};

/// Web 初次连接时读取的 Editor 能力声明。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EditorCapabilities {
    pub protocol_version: u32,
    pub max_scene_page_size: u16,
    pub editable_material_fields: Vec<String>,
}

/// 场景对象列表中的轻量 instance 摘要。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SceneObjectSummary {
    /// 当前 session 内用于查询和稳定列表 key 的 opaque identity。
    pub instance_id: InstanceId,

    /// `SceneStore` 持有的展示名称；名称不要求唯一，不能替代 `instance_id`。
    pub name: String,

    /// instance-local material binding 数量，与 mesh submesh 数量保持一致。
    pub material_count: u32,
}

/// Instance 详情中的 CPU scene mesh 引用。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MeshSummaryDto {
    /// 当前 session 内的 mesh opaque identity，不表示 GPU geometry slot 或 BLAS identity。
    pub mesh_id: MeshId,

    /// `SceneStore` 长期保存的 mesh 展示名称。
    pub name: String,
}

/// Instance 详情中的一条 submesh/material binding。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InstanceMaterialBindingDto {
    /// Instance-local submesh 顺序；同一索引同时选择 mesh geometry 和 material。
    pub submesh_index: u32,

    /// 当前 session 内的 material opaque identity。
    pub material_id: MaterialId,

    /// CPU `MaterialData` 中的展示名称。
    pub name: String,
}

/// Web 右侧栏按需读取的完整 CPU scene instance 投影。
///
/// DTO 在一次 Renderer update 查询中从同一个 `SceneReadView` 构造，Web 只把它作为
/// 可丢弃投影。`transform` 使用 row-major 行数组，避免页面误解 glam 的 column-major
/// 内存表达；它不属于 shader ABI，也不表示 GPU scene 已经 ready。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct InstanceDetailsDto {
    /// 构造详情时对应的 CPU scene 全局版本。
    pub scene_version: SceneVersion,

    /// 当前 session 内用于重新查询和失效检测的 opaque identity。
    pub instance_id: InstanceId,

    /// `SceneStore` 持有的 instance 展示名称。
    pub name: String,

    /// CPU world transform 的四行矩阵，外层索引为 row，内层索引为 column。
    pub transform: [[f32; 4]; 4],

    /// Instance 引用的唯一 CPU scene mesh。
    pub mesh: MeshSummaryDto,

    /// 按 `submesh_index` 排列的 material bindings。
    pub materials: Vec<InstanceMaterialBindingDto>,
}

/// 单页场景对象查询结果。
///
/// Web 必须比较每页 version；分页过程中 version 改变时丢弃结果并重新从 offset 0 查询。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SceneObjectsPage {
    pub scene_version: SceneVersion,
    pub objects: Vec<SceneObjectSummary>,
    pub next_offset: Option<u32>,
}
