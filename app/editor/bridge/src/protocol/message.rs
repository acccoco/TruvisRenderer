use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::protocol::{
    EditorError, InstanceDetailsDto, InstanceId, MaterialDto, MaterialId, MaterialPatch, SceneObjectsPage,
    SceneVersion, SelectionDto,
};

/// 不修改 World 的 Editor 查询。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(tag = "type", rename_all = "snake_case")]
pub enum EditorQuery {
    GetSceneVersion,
    GetSelection,
    GetSceneObjects {
        offset: u32,
        limit: u16,
        expected_scene_version: Option<SceneVersion>,
    },
    GetInstanceDetails {
        instance_id: InstanceId,
    },
    GetMaterial {
        material_id: MaterialId,
    },
}

/// 修改 CPU World 权威状态的 Editor 命令。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(tag = "type", rename_all = "snake_case")]
pub enum EditorCommand {
    UpdateMaterial {
        material_id: MaterialId,
        patch: MaterialPatch,
    },
}

/// Web 发送的领域请求分类。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "category", content = "payload", rename_all = "snake_case")]
#[ts(tag = "category", content = "payload", rename_all = "snake_case")]
pub enum EditorRequest {
    Query(EditorQuery),
    Command(EditorCommand),
}

/// Renderer 对指定请求返回的领域结果。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
#[ts(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum EditorResponse {
    SceneVersion(SceneVersion),
    Selection(Option<SelectionDto>),
    SceneObjects(SceneObjectsPage),
    InstanceDetails(InstanceDetailsDto),
    Material(MaterialDto),
    CommandApplied {
        scene_version: SceneVersion,
        material: MaterialDto,
    },
    Error(EditorError),
}

/// Renderer 主动发送的 best-effort 通知。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
#[ts(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum EditorNotification {
    SelectionChanged(Option<SelectionDto>),
    SceneVersionChanged(SceneVersion),
}
