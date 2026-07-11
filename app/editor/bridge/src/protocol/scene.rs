use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::protocol::{InstanceId, SceneVersion};

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
    pub instance_id: InstanceId,
    pub material_count: u32,
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
