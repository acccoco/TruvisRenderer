use std::path::PathBuf;
use std::time::{Duration, Instant};

use truvis_editor_bridge::protocol::{
    AutomationCommand, AutomationQuery, AutomationRequest, AutomationResponse, EditorCommand, EditorError,
    EditorErrorCode, EditorQuery, EditorResponse, InstanceTransformDto, SceneImportStatusDto, SceneSummaryDto,
    SceneVersion,
};
use truvis_editor_bridge::{AutomationRendererEndpoint, RequestEnvelope};
use truvis_world::GameWorld;
use truvis_world::components::transform::TransformTrs;
use truvis_world::guid_new_type::SceneImportHandle;

use super::editor_controller::WorldSceneAdapter;

#[derive(Clone, Copy, Debug)]
pub(crate) struct WorldAutomationControllerConfig {
    pub(crate) max_requests_per_frame: usize,
    pub(crate) max_time_per_frame: Duration,
}

impl Default for WorldAutomationControllerConfig {
    fn default() -> Self {
        Self {
            max_requests_per_frame: 16,
            max_time_per_frame: Duration::from_micros(500),
        }
    }
}

pub(crate) struct WorldAutomationController {
    endpoint: AutomationRendererEndpoint,
    config: WorldAutomationControllerConfig,
}

impl WorldAutomationController {
    pub(crate) fn new(endpoint: AutomationRendererEndpoint) -> Self {
        Self {
            endpoint,
            config: WorldAutomationControllerConfig::default(),
        }
    }

    pub(crate) fn process_requests(&mut self, world: &mut GameWorld) {
        let started = Instant::now();
        for _ in 0..self.config.max_requests_per_frame {
            if started.elapsed() >= self.config.max_time_per_frame {
                break;
            }
            let envelope = match self.endpoint.try_receive_request() {
                Ok(envelope) => envelope,
                Err(
                    tokio::sync::mpsc::error::TryRecvError::Empty
                    | tokio::sync::mpsc::error::TryRecvError::Disconnected,
                ) => break,
            };
            self.process_request(world, envelope);
        }
    }

    fn process_request(&self, world: &mut GameWorld, envelope: RequestEnvelope<AutomationRequest, AutomationResponse>) {
        let response = match envelope.request {
            AutomationRequest::Query(query) => self.query(world, query),
            AutomationRequest::Command(command) => self.command(world, command),
        };
        let _ = envelope.reply.send(response);
    }

    fn query(&self, world: &GameWorld, query: AutomationQuery) -> AutomationResponse {
        match query {
            AutomationQuery::GetSceneSummary => {
                let view = world.scene_view();
                AutomationResponse::SceneSummary(SceneSummaryDto {
                    scene_version: SceneVersion::from_u64(view.scene_version()),
                    object_count: view.instance_map().len() as u32,
                    material_count: view.material_handles().count() as u32,
                    mesh_count: view.mesh_handles().count() as u32,
                })
            }
            AutomationQuery::GetSceneObjects {
                offset,
                limit,
                expected_scene_version,
            } => Self::map_editor_response(WorldSceneAdapter::handle_query(
                world,
                None,
                EditorQuery::GetSceneObjects {
                    offset,
                    limit,
                    expected_scene_version,
                },
            )),
            AutomationQuery::GetInstanceDetails { instance_id } => Self::map_editor_response(
                WorldSceneAdapter::handle_query(world, None, EditorQuery::GetInstanceDetails { instance_id }),
            ),
            AutomationQuery::GetMaterial { material_id } => Self::map_editor_response(WorldSceneAdapter::handle_query(
                world,
                None,
                EditorQuery::GetMaterial { material_id },
            )),
            AutomationQuery::GetSceneImportStatus { import_id } => self.import_status(world, import_id),
        }
    }

    fn command(&self, world: &mut GameWorld, command: AutomationCommand) -> AutomationResponse {
        match command {
            AutomationCommand::UpdateMaterial {
                material_id,
                patch,
                expected_scene_version,
            } => {
                if let Some(error) = Self::check_version(world, expected_scene_version) {
                    return AutomationResponse::Error(error);
                }
                Self::map_editor_response(
                    WorldSceneAdapter::handle_command(world, EditorCommand::UpdateMaterial { material_id, patch }).0,
                )
            }
            AutomationCommand::UpdateTransform {
                instance_id,
                transform,
                expected_scene_version,
            } => {
                if let Some(error) = Self::check_version(world, expected_scene_version) {
                    return AutomationResponse::Error(error);
                }
                let handle = match WorldSceneAdapter::decode_instance_id(&instance_id) {
                    Ok(handle) => handle,
                    Err(error) => return AutomationResponse::Error(error),
                };
                let matrix = match Self::transform_matrix(transform) {
                    Ok(matrix) => matrix,
                    Err(error) => return AutomationResponse::Error(error),
                };
                if world.scene_view().get_instance(handle).is_none() {
                    return AutomationResponse::Error(EditorError::new(
                        EditorErrorCode::StaleObject,
                        "instance ID is no longer valid",
                    ));
                }
                if let Err(error) = world.update_instance_transform(handle, matrix) {
                    return AutomationResponse::Error(EditorError::new(
                        EditorErrorCode::InvalidRequest,
                        format!("transform update failed: {error}"),
                    ));
                }
                AutomationResponse::CommandApplied {
                    scene_version: SceneVersion::from_u64(world.scene_view().scene_version()),
                    material: None,
                }
            }
            AutomationCommand::ImportScene {
                path,
                expected_scene_version,
            } => {
                if let Some(error) = Self::check_version(world, expected_scene_version) {
                    return AutomationResponse::Error(error);
                }
                let handle = world.import_scene(PathBuf::from(path));
                AutomationResponse::SceneImportAccepted {
                    import_id: WorldSceneAdapter::encode_key("import", handle),
                    scene_version: SceneVersion::from_u64(world.scene_view().scene_version()),
                }
            }
        }
    }

    fn import_status(&self, world: &GameWorld, import_id: String) -> AutomationResponse {
        let handle = match WorldSceneAdapter::decode_key::<SceneImportHandle>("import", &import_id) {
            Ok(handle) => handle,
            Err(error) => return AutomationResponse::Error(error),
        };
        let status = world.scene_import_status(handle);
        let status_name = match status {
            truvis_asset::handle::LoadStatus::Unloaded => "unloaded",
            truvis_asset::handle::LoadStatus::Loading => "loading",
            truvis_asset::handle::LoadStatus::Ready => "ready",
            truvis_asset::handle::LoadStatus::Failed => "failed",
        };
        AutomationResponse::SceneImportStatus(SceneImportStatusDto {
            import_id,
            status: status_name.to_string(),
            error: world.scene_import_error(handle).map(str::to_string),
        })
    }

    fn check_version(world: &GameWorld, expected: Option<SceneVersion>) -> Option<EditorError> {
        let Some(expected) = expected else {
            return None;
        };
        let expected = match expected.parse() {
            Ok(value) => value,
            Err(_) => {
                return Some(EditorError::new(EditorErrorCode::InvalidRequest, "scene version is not a u64 string"));
            }
        };
        (expected != world.scene_view().scene_version())
            .then(|| EditorError::new(EditorErrorCode::Conflict, "scene version conflict"))
    }

    fn transform_matrix(transform: InstanceTransformDto) -> Result<glam::Mat4, EditorError> {
        if transform
            .location
            .iter()
            .chain(transform.rotation_degrees.iter())
            .chain(transform.scale.iter())
            .any(|value| !value.is_finite())
        {
            return Err(EditorError::new(EditorErrorCode::InvalidRequest, "transform values must be finite"));
        }
        let rotation = glam::Quat::from_euler(
            glam::EulerRot::XYZ,
            transform.rotation_degrees[0].to_radians(),
            transform.rotation_degrees[1].to_radians(),
            transform.rotation_degrees[2].to_radians(),
        );
        let trs = TransformTrs {
            translation: transform.location.into(),
            rotation,
            scale: transform.scale.into(),
        };
        Ok(trs.to_matrix())
    }

    fn map_editor_response(response: EditorResponse) -> AutomationResponse {
        match response {
            EditorResponse::SceneObjects(value) => AutomationResponse::SceneObjects(value),
            EditorResponse::InstanceDetails(value) => AutomationResponse::InstanceDetails(value),
            EditorResponse::Material(value) => AutomationResponse::Material(value),
            EditorResponse::CommandApplied {
                scene_version,
                material,
            } => AutomationResponse::CommandApplied {
                scene_version,
                material: Some(material),
            },
            EditorResponse::Error(error) => AutomationResponse::Error(error),
            EditorResponse::SceneVersion(_) | EditorResponse::Selection(_) => {
                AutomationResponse::Error(EditorError::new(EditorErrorCode::Internal, "unexpected editor response"))
            }
        }
    }

    pub(crate) fn shutdown(&mut self) {
        self.endpoint.shutdown();
    }
}
