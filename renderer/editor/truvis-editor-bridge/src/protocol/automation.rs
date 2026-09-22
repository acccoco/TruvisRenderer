use serde::{Deserialize, Serialize};

use super::{
    EditorError, InstanceDetailsDto, InstanceId, InstanceTransformDto, MaterialDto, MaterialId, MaterialPatch,
    SceneObjectsPage, SceneVersion,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationQuery {
    GetSceneSummary,
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
    GetSceneImportStatus {
        import_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationCommand {
    UpdateTransform {
        instance_id: InstanceId,
        transform: InstanceTransformDto,
        expected_scene_version: Option<SceneVersion>,
    },
    UpdateMaterial {
        material_id: MaterialId,
        patch: MaterialPatch,
        expected_scene_version: Option<SceneVersion>,
    },
    ImportScene {
        path: String,
        expected_scene_version: Option<SceneVersion>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "category", content = "payload", rename_all = "snake_case")]
pub enum AutomationRequest {
    Query(AutomationQuery),
    Command(AutomationCommand),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum AutomationResponse {
    SceneSummary(SceneSummaryDto),
    SceneObjects(SceneObjectsPage),
    InstanceDetails(InstanceDetailsDto),
    Material(MaterialDto),
    CommandApplied {
        scene_version: SceneVersion,
        material: Option<MaterialDto>,
    },
    SceneImportAccepted {
        import_id: String,
        scene_version: SceneVersion,
    },
    SceneImportStatus(SceneImportStatusDto),
    Error(EditorError),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneSummaryDto {
    pub scene_version: SceneVersion,
    pub object_count: u32,
    pub material_count: u32,
    pub mesh_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneImportStatusDto {
    pub import_id: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AutomationNotification {
    SceneVersionChanged(SceneVersion),
}
