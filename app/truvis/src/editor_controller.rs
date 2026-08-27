use std::time::{Duration, Instant};

use slotmap::{Key, KeyData};

use truvis_editor_bridge::protocol::{
    CoverageModeDto, DEFAULT_SCENE_PAGE_SIZE, EditorCommand, EditorError, EditorErrorCode, EditorNotification,
    EditorQuery, EditorRequest, EditorResponse, InstanceDetailsDto, InstanceId, InstanceMaterialBindingDto,
    MAX_SCENE_PAGE_SIZE, MaterialClassDto, MaterialDto, MaterialId, MaterialPatch, MeshId, MeshSummaryDto,
    SceneObjectSummary, SceneObjectsPage, SceneVersion, SelectionDto, TextureId,
};
use truvis_editor_bridge::{AppEndpoint, EditorRequestEnvelope};
use truvis_render_runtime::selection::WorldSubmeshSelection;
use truvis_world::World;
use truvis_world::components::material::{CoverageMode, MaterialClass, MaterialData};
use truvis_world::guid_new_type::{InstanceHandle, MaterialHandle, MeshHandle, TextureHandle};

/// Editor 请求在单帧 update 中的处理预算。
///
/// 消息数与 wall-clock 时间使用双重上限，避免复杂 Query 在请求洪峰下无限拉长 Render 帧。
#[derive(Clone, Copy, Debug)]
pub(crate) struct EditorControllerConfig {
    max_requests_per_frame: usize,
    max_time_per_frame: Duration,
}

impl Default for EditorControllerConfig {
    fn default() -> Self {
        Self {
            max_requests_per_frame: 32,
            max_time_per_frame: Duration::from_micros(500),
        }
    }
}

/// `TruvisRenderer` 内的 Editor 协议适配器。
///
/// Controller 只在 RenderThread 的 Renderer update 阶段借用 `World`，把协议 DTO 转换成现有
/// World 查询或 mutation。它不保存 selection、scene/material cache，也不拥有 Desktop IPC 生命周期。
pub(crate) struct EditorController {
    endpoint: AppEndpoint,
    config: EditorControllerConfig,
}

impl EditorController {
    pub(crate) fn new(endpoint: AppEndpoint, config: EditorControllerConfig) -> Self {
        Self { endpoint, config }
    }

    /// 按单帧预算处理 Query / Command。
    pub(crate) fn process_requests(&mut self, world: &mut World, selection: Option<WorldSubmeshSelection>) {
        let started_at = Instant::now();
        for _ in 0..self.config.max_requests_per_frame {
            if started_at.elapsed() >= self.config.max_time_per_frame {
                break;
            }
            let envelope = match self.endpoint.try_receive_request() {
                Ok(envelope) => envelope,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
            };
            self.process_request(world, selection, envelope);
        }
    }

    fn process_request(
        &self,
        world: &mut World,
        selection: Option<WorldSubmeshSelection>,
        envelope: EditorRequestEnvelope,
    ) {
        let EditorRequestEnvelope { request, reply } = envelope;
        let (response, notification) = match request {
            EditorRequest::Query(query) => (Self::handle_query(world, selection, query), None),
            EditorRequest::Command(command) => Self::handle_command(world, command),
        };

        // WebView 可能已刷新、timeout 或进入 shutdown；reply receiver 消失不能回滚已经
        // 完成的 World mutation，因此 send 失败只表示结果无人接收。
        let _ = reply.send(response);
        if let Some(notification) = notification {
            let _ = self.endpoint.try_send_notification(notification);
        }
    }

    fn handle_query(world: &World, selection: Option<WorldSubmeshSelection>, query: EditorQuery) -> EditorResponse {
        match query {
            EditorQuery::GetSceneVersion => {
                EditorResponse::SceneVersion(SceneVersion::from_u64(world.scene_view().scene_version()))
            }
            EditorQuery::GetSelection => EditorResponse::Selection(Self::selection_dto(world, selection)),
            EditorQuery::GetSceneObjects {
                offset,
                limit,
                expected_scene_version,
            } => Self::scene_objects_page(world, offset, limit, expected_scene_version),
            EditorQuery::GetInstanceDetails { instance_id } => Self::instance_details(world, instance_id),
            EditorQuery::GetMaterial { material_id } => match Self::decode_material_id(&material_id) {
                Ok(handle) => match Self::material_dto(world, handle) {
                    Some(material) => EditorResponse::Material(material),
                    None => Self::error(EditorErrorCode::StaleObject, "material ID is no longer valid"),
                },
                Err(error) => EditorResponse::Error(error),
            },
        }
    }

    fn handle_command(world: &mut World, command: EditorCommand) -> (EditorResponse, Option<EditorNotification>) {
        match command {
            EditorCommand::UpdateMaterial { material_id, patch } => {
                let handle = match Self::decode_material_id(&material_id) {
                    Ok(handle) => handle,
                    Err(error) => return (EditorResponse::Error(error), None),
                };
                let Some(current) = world.material_data(handle).cloned() else {
                    return (Self::error(EditorErrorCode::StaleObject, "material ID is no longer valid"), None);
                };
                let updated = match Self::apply_material_patch(current, patch) {
                    Ok(updated) => updated,
                    Err(error) => return (EditorResponse::Error(error), None),
                };
                let previous_scene_version = world.scene_view().scene_version();
                if let Err(error) = world.update_material(handle, updated) {
                    return (
                        Self::error(EditorErrorCode::InvalidRequest, format!("material update failed: {error}")),
                        None,
                    );
                }

                let current_scene_version = world.scene_view().scene_version();
                let scene_version = SceneVersion::from_u64(current_scene_version);
                let Some(material) = Self::material_dto(world, handle) else {
                    return (Self::error(EditorErrorCode::Internal, "updated material disappeared"), None);
                };
                let notification = (current_scene_version != previous_scene_version)
                    .then(|| EditorNotification::SceneVersionChanged(scene_version.clone()));
                (
                    EditorResponse::CommandApplied {
                        scene_version: scene_version.clone(),
                        material,
                    },
                    notification,
                )
            }
        }
    }
}

impl EditorController {
    fn scene_objects_page(
        world: &World,
        offset: u32,
        limit: u16,
        expected_scene_version: Option<SceneVersion>,
    ) -> EditorResponse {
        let view = world.scene_view();
        let scene_version = view.scene_version();
        if let Some(expected) = expected_scene_version {
            let expected = match expected.parse() {
                Ok(expected) => expected,
                Err(_) => return Self::error(EditorErrorCode::InvalidRequest, "scene version is not a u64 string"),
            };
            if expected != scene_version {
                return Self::error(EditorErrorCode::Conflict, "scene changed while object pages were being read");
            }
        }

        let limit = if limit == 0 { DEFAULT_SCENE_PAGE_SIZE } else { limit.min(MAX_SCENE_PAGE_SIZE) } as usize;
        let offset = offset as usize;
        let instances = view.instance_map();
        let objects = instances
            .iter()
            .skip(offset)
            .take(limit)
            .map(|(handle, instance)| SceneObjectSummary {
                instance_id: Self::encode_instance_id(handle),
                name: instance.name.clone(),
                material_count: instance.materials.len() as u32,
            })
            .collect::<Vec<_>>();
        let consumed = offset.saturating_add(objects.len());
        let next_offset = (consumed < instances.len()).then_some(consumed as u32);

        EditorResponse::SceneObjects(SceneObjectsPage {
            scene_version: SceneVersion::from_u64(scene_version),
            objects,
            next_offset,
        })
    }

    /// 从一次 CPU scene 只读快照构造 Web inspector 的 owned instance 详情。
    ///
    /// Instance 对 mesh/material 的引用完整性由 `SceneStore` 在注册、更新和删除边界维护；
    /// 因此 live instance 出现缺失依赖表示内部不变量已经破坏，而不是普通 stale query。
    fn instance_details(world: &World, instance_id: InstanceId) -> EditorResponse {
        let handle = match Self::decode_instance_id(&instance_id) {
            Ok(handle) => handle,
            Err(error) => return EditorResponse::Error(error),
        };
        let view = world.scene_view();
        let Some(instance) = view.get_instance(handle) else {
            return Self::error(EditorErrorCode::StaleObject, "instance ID is no longer valid");
        };
        let Some(mesh_name) = view.mesh_name(instance.mesh) else {
            return Self::error(EditorErrorCode::Internal, "live instance references a missing mesh");
        };

        let mut materials = Vec::with_capacity(instance.materials.len());
        for (submesh_index, material_handle) in instance.materials.iter().copied().enumerate() {
            let Some(material) = view.material_data(material_handle) else {
                return Self::error(EditorErrorCode::Internal, "live instance references a missing material");
            };
            materials.push(InstanceMaterialBindingDto {
                submesh_index: submesh_index as u32,
                material_id: Self::encode_material_id(material_handle),
                name: material.name.clone(),
            });
        }

        // glam 使用 column-major 存储；先 transpose 再导出 columns，使 wire DTO 的外层数组明确表示 matrix rows。
        let transform = instance.transform.transpose().to_cols_array_2d();
        EditorResponse::InstanceDetails(InstanceDetailsDto {
            scene_version: SceneVersion::from_u64(view.scene_version()),
            instance_id,
            name: instance.name.clone(),
            transform,
            mesh: MeshSummaryDto {
                mesh_id: Self::encode_mesh_id(instance.mesh),
                name: mesh_name.to_string(),
            },
            materials,
        })
    }

    fn selection_dto(world: &World, selection: Option<WorldSubmeshSelection>) -> Option<SelectionDto> {
        let selection = selection?;
        let instance = world.scene_view().get_instance(selection.instance)?;
        let material = *instance.materials.get(selection.submesh_index as usize)?;
        Some(Self::selection_dto_from_handles(selection.instance, selection.submesh_index, material))
    }

    fn material_dto(world: &World, handle: MaterialHandle) -> Option<MaterialDto> {
        let data = world.material_data(handle)?;
        Some(MaterialDto {
            id: Self::encode_material_id(handle),
            name: data.name.clone(),
            base_color: data.base_color.to_array(),
            metallic: data.metallic,
            roughness: data.roughness,
            class: Self::material_class_dto(data.class),
            coverage: Self::coverage_dto(data.coverage),
            diffuse_texture: data.diffuse_texture.map(Self::encode_texture_id),
            normal_texture: data.normal_texture.map(Self::encode_texture_id),
        })
    }

    fn apply_material_patch(mut data: MaterialData, patch: MaterialPatch) -> Result<MaterialData, EditorError> {
        if let Some(name) = patch.name {
            let name = name.trim();
            if name.is_empty() || name.len() > 256 {
                return Err(EditorError::new(
                    EditorErrorCode::InvalidRequest,
                    "material name must contain 1 to 256 UTF-8 bytes",
                ));
            }
            data.name = name.to_string();
        }
        if let Some(base_color) = patch.base_color {
            if base_color.iter().any(|value| !value.is_finite())
                || base_color[..3].iter().any(|value| *value < 0.0)
                || !(0.0..=1.0).contains(&base_color[3])
            {
                return Err(EditorError::new(
                    EditorErrorCode::InvalidRequest,
                    "base color must be finite, RGB must be non-negative, and alpha must be in [0, 1]",
                ));
            }
            data.base_color = glam::Vec4::from_array(base_color);
        }
        if let Some(metallic) = patch.metallic {
            Self::validate_unit_value("metallic", metallic)?;
            data.metallic = metallic;
        }
        if let Some(roughness) = patch.roughness {
            Self::validate_unit_value("roughness", roughness)?;
            data.roughness = roughness;
        }
        if let Some(class) = patch.class {
            data.class = Self::material_class(class)?;
        }
        if let Some(coverage) = patch.coverage {
            data.coverage = Self::coverage(coverage)?;
        }
        Ok(data)
    }

    fn validate_unit_value(name: &str, value: f32) -> Result<(), EditorError> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(EditorError::new(
                EditorErrorCode::InvalidRequest,
                format!("{name} must be finite and in [0, 1]"),
            ));
        }
        Ok(())
    }
}

impl EditorController {
    fn material_class(dto: MaterialClassDto) -> Result<MaterialClass, EditorError> {
        match dto {
            MaterialClassDto::Surface => Ok(MaterialClass::Surface),
            MaterialClassDto::Transmission { opacity, ior } => {
                Self::validate_unit_value("transmission opacity", opacity)?;
                if !ior.is_finite() || ior < 1.0 {
                    return Err(EditorError::new(
                        EditorErrorCode::InvalidRequest,
                        "transmission IOR must be finite and at least 1",
                    ));
                }
                Ok(MaterialClass::Transmission { opacity, ior })
            }
            MaterialClassDto::Emissive { radiance } => {
                if radiance.iter().any(|value| !value.is_finite() || *value < 0.0) {
                    return Err(EditorError::new(
                        EditorErrorCode::InvalidRequest,
                        "emissive radiance must contain finite non-negative values",
                    ));
                }
                Ok(MaterialClass::Emissive {
                    radiance: glam::Vec3::from_array(radiance),
                })
            }
        }
    }

    fn material_class_dto(class: MaterialClass) -> MaterialClassDto {
        match class {
            MaterialClass::Surface => MaterialClassDto::Surface,
            MaterialClass::Transmission { opacity, ior } => MaterialClassDto::Transmission { opacity, ior },
            MaterialClass::Emissive { radiance } => MaterialClassDto::Emissive {
                radiance: radiance.to_array(),
            },
        }
    }

    fn coverage(dto: CoverageModeDto) -> Result<CoverageMode, EditorError> {
        match dto {
            CoverageModeDto::Opaque => Ok(CoverageMode::Opaque),
            CoverageModeDto::AlphaMask { alpha_cutoff } => {
                Self::validate_unit_value("alpha cutoff", alpha_cutoff)?;
                Ok(CoverageMode::AlphaMask { alpha_cutoff })
            }
        }
    }

    fn coverage_dto(coverage: CoverageMode) -> CoverageModeDto {
        match coverage {
            CoverageMode::Opaque => CoverageModeDto::Opaque,
            CoverageMode::AlphaMask { alpha_cutoff } => CoverageModeDto::AlphaMask { alpha_cutoff },
        }
    }
}

impl EditorController {
    /// 广播由 Editor command 之外的 Renderer-local mutation 产生的 scene version 变化。
    ///
    /// 本方法只复用现有失效通知，不携带本地文件路径或新的领域 DTO。notification
    /// outbox 是 best-effort；发送失败时 Web 仍会通过既有一秒 polling 收敛。
    pub(crate) fn notify_scene_version_changed(&self, scene_version: u64) {
        let _ = self
            .endpoint
            .try_send_notification(EditorNotification::SceneVersionChanged(SceneVersion::from_u64(scene_version)));
    }

    pub(crate) fn notify_selection_changed(&self, selection: Option<(InstanceHandle, u32, MaterialHandle)>) {
        let selection = selection.map(|(instance, submesh_index, material)| {
            Self::selection_dto_from_handles(instance, submesh_index, material)
        });
        let _ = self.endpoint.try_send_notification(EditorNotification::SelectionChanged(selection));
    }

    fn selection_dto_from_handles(
        instance: InstanceHandle,
        submesh_index: u32,
        material: MaterialHandle,
    ) -> SelectionDto {
        SelectionDto {
            instance_id: Self::encode_instance_id(instance),
            submesh_index,
            material_id: Self::encode_material_id(material),
        }
    }

    pub(crate) fn shutdown(&mut self) {
        self.endpoint.shutdown();
    }
}

impl EditorController {
    fn encode_instance_id(handle: InstanceHandle) -> InstanceId {
        InstanceId::new(Self::encode_key("instance", handle))
    }

    fn encode_material_id(handle: MaterialHandle) -> MaterialId {
        MaterialId::new(Self::encode_key("material", handle))
    }

    fn encode_mesh_id(handle: MeshHandle) -> MeshId {
        MeshId::new(Self::encode_key("mesh", handle))
    }

    fn encode_texture_id(handle: TextureHandle) -> TextureId {
        TextureId::new(Self::encode_key("texture", handle))
    }

    fn encode_key<K: Key>(prefix: &str, handle: K) -> String {
        format!("{prefix}:{:016x}", handle.data().as_ffi())
    }

    fn decode_material_id(id: &MaterialId) -> Result<MaterialHandle, EditorError> {
        Self::decode_key("material", &id.0)
    }

    fn decode_instance_id(id: &InstanceId) -> Result<InstanceHandle, EditorError> {
        Self::decode_key("instance", &id.0)
    }

    fn decode_key<K: Key>(expected_prefix: &str, value: &str) -> Result<K, EditorError> {
        let Some((prefix, raw)) = value.split_once(':') else {
            return Err(EditorError::new(EditorErrorCode::InvalidRequest, "editor ID is missing its type prefix"));
        };
        if prefix != expected_prefix {
            return Err(EditorError::new(
                EditorErrorCode::InvalidRequest,
                format!("expected {expected_prefix} ID, got {prefix}"),
            ));
        }
        let raw = u64::from_str_radix(raw, 16)
            .map_err(|_| EditorError::new(EditorErrorCode::InvalidRequest, "editor ID contains invalid hex data"))?;
        Ok(K::from(KeyData::from_ffi(raw)))
    }

    fn error(code: EditorErrorCode, message: impl Into<String>) -> EditorResponse {
        EditorResponse::Error(EditorError::new(code, message))
    }
}
