use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::protocol::{
    EditorCapabilities, EditorError, InstanceDetailsDto, InstanceId, MaterialDto, MaterialId, MaterialPatch, RequestId,
    SceneObjectsPage, SceneVersion, SelectionDto,
};

/// 不修改 World 的 Editor 查询。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(tag = "type", rename_all = "snake_case")]
pub enum EditorQuery {
    GetCapabilities,
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

/// WebSocket client 发出的完整消息 envelope。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct EditorClientMessage {
    pub protocol_version: u32,
    pub request_id: RequestId,
    pub request: EditorRequest,
}

/// Render/App 对指定请求返回的领域结果。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
#[ts(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum EditorResponse {
    Capabilities(EditorCapabilities),
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

/// Render/App 主动发送的 best-effort 通知。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
#[ts(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum EditorNotification {
    SelectionChanged(Option<SelectionDto>),
    SceneVersionChanged(SceneVersion),
}

/// Server 发给 WebSocket client 的完整消息。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(tag = "kind", rename_all = "snake_case")]
pub enum EditorServerMessage {
    Response {
        request_id: RequestId,
        response: EditorResponse,
    },
    Notification {
        notification: EditorNotification,
    },
}
