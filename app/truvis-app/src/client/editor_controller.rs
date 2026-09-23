use std::time::{Duration, Instant};

use slotmap::{Key, KeyData};
use truvis_asset::material_texture::{TextureFilter, TextureSampler, TextureTransform, TextureWrap};
use truvis_editor_bridge::{
    EditorRendererEndpoint, EditorRequestEnvelope,
    protocol::{
        AreaLightShapeDto, CoverageModeDto, DEFAULT_SCENE_PAGE_SIZE, EditorCommand, EditorError, EditorErrorCode,
        EditorNotification, EditorQuery, EditorRequest, EditorResponse, EnvironmentDetailsDto, EnvironmentLoadState,
        InstanceDetailsDto, InstanceId, InstanceMaterialBindingDto, InstanceTransformDto, LightDetailsDto, LightId,
        LightKindDto, LightParametersDto, MAX_SCENE_PAGE_SIZE, MaterialClassDto, MaterialDto, MaterialId,
        MaterialPatch, MeshId, MeshSummaryDto, SceneObjectSummary, SceneObjectsPage, SceneVersion, SelectionDto,
        TextureId, TextureMappingDto, TextureSlotDto,
    },
};
use truvis_renderer::{SceneSelection, SelectionChange};
use truvis_world::{
    AreaLightShape, AssetSource, GameWorld, LightTarget, SceneEditError, TextureState, WorldEditError,
    components::material::{CoverageMode, MaterialClass, MaterialData},
    guid_new_type::{MaterialAssetHandle, MeshAssetHandle, MeshInstanceHandle, TextureAssetHandle},
};

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

/// `TruvisAppClient` 内的 Editor 协议适配器。
///
/// Controller 只在 RenderThread 的 Renderer update 阶段借用 `GameWorld`，把协议 DTO 转换成现有
/// GameWorld 查询或 mutation。它不保存 selection、scene/material cache，也不拥有 Desktop IPC 生命周期。
pub(crate) struct EditorController {
    endpoint: EditorRendererEndpoint,
    config: EditorControllerConfig,
}

/// Editor 与 Automation 共用的 CPU scene DTO、opaque ID 和 mutation 校验适配器。
///
/// 它不保存 scene cache；所有调用都在 RenderThread update 阶段借用当前 `GameWorld`。
pub(crate) struct WorldSceneAdapter;

impl EditorController {
    pub(crate) fn new(endpoint: EditorRendererEndpoint, config: EditorControllerConfig) -> Self {
        Self { endpoint, config }
    }

    /// 按单帧预算处理 Query / Command。
    pub(crate) fn process_requests(&mut self, world: &mut GameWorld, selection: Option<SceneSelection>) {
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
        world: &mut GameWorld,
        selection: Option<SceneSelection>,
        envelope: EditorRequestEnvelope,
    ) {
        let EditorRequestEnvelope { request, reply } = envelope;
        let (response, notification) = match request {
            EditorRequest::Query(query) => (WorldSceneAdapter::handle_query(world, selection, query), None),
            EditorRequest::Command(command) => WorldSceneAdapter::handle_command(world, command),
        };

        // WebView 可能已刷新、timeout 或进入 shutdown；reply receiver 消失不能回滚已经
        // 完成的 GameWorld mutation，因此 send 失败只表示结果无人接收。
        let _ = reply.send(response);
        if let Some(notification) = notification {
            let _ = self.endpoint.try_send_notification(notification);
        }
    }
}

impl WorldSceneAdapter {
    fn light_identity(target: LightTarget) -> (LightId, LightKindDto) {
        let (prefix, handle, kind) = match target {
            LightTarget::Point(handle) => ("point", handle, LightKindDto::Point),
            LightTarget::Spot(handle) => ("spot", handle, LightKindDto::Spot),
            LightTarget::Area(handle) => ("area", handle, LightKindDto::Area),
        };
        (LightId(Self::encode_key(prefix, handle)), kind)
    }

    fn light_selection_dto(target: LightTarget) -> SelectionDto {
        let (light_id, kind) = Self::light_identity(target);
        SelectionDto::Light { light_id, kind }
    }

    fn decode_light_id(light_id: &LightId) -> Result<LightTarget, EditorError> {
        match light_id.0.split_once(':').map(|(prefix, _)| prefix) {
            Some("point") => Self::decode_key("point", &light_id.0).map(LightTarget::Point),
            Some("spot") => Self::decode_key("spot", &light_id.0).map(LightTarget::Spot),
            Some("area") => Self::decode_key("area", &light_id.0).map(LightTarget::Area),
            _ => Err(EditorError::new(EditorErrorCode::InvalidRequest, "invalid light ID kind")),
        }
    }

    fn light_dto(world: &GameWorld, light_id: LightId) -> Result<LightDetailsDto, EditorError> {
        let target = Self::decode_light_id(&light_id)?;
        let scene = world.scene_view();
        let stale = || EditorError::new(EditorErrorCode::StaleObject, "light ID is no longer valid");
        let position = scene.light_position(target).ok_or_else(stale)?.to_array();
        let (radiance, parameters) = match target {
            LightTarget::Point(id) => {
                let light = scene.point_light_map().get(id).ok_or_else(stale)?;
                (glam::Vec3::from(light.color).to_array(), LightParametersDto::Point)
            }
            LightTarget::Spot(id) => {
                let light = scene.spot_light_map().get(id).ok_or_else(stale)?;
                (
                    glam::Vec3::from(light.color).to_array(),
                    LightParametersDto::Spot {
                        direction: glam::Vec3::from(light.dir).to_array(),
                        inner_angle_degrees: light.inner_angle.to_degrees(),
                        outer_angle_degrees: light.outer_angle.to_degrees(),
                    },
                )
            }
            LightTarget::Area(id) => {
                let light = scene.area_light_map().get(id).ok_or_else(stale)?;
                (
                    glam::Vec3::from(light.radiance).to_array(),
                    LightParametersDto::Area {
                        shape: AreaLightShape::from_axes(light.half_u.into(), light.half_v.into()).map(|shape| {
                            AreaLightShapeDto {
                                rotation_degrees: shape.rotation_degrees.to_array(),
                                width: shape.width,
                                height: shape.height,
                            }
                        }),
                    },
                )
            }
        };
        Ok(LightDetailsDto {
            scene_version: SceneVersion::from_u64(scene.scene_version()),
            light_id,
            position,
            radiance,
            parameters,
        })
    }

    fn environment_dto(world: &GameWorld) -> EnvironmentDetailsDto {
        let scene = world.scene_view();
        let sky = scene.sky_state();
        let record = sky.texture.and_then(|id| scene.texture_record(id));
        let file_name = record.map(|record| match &record.source {
            AssetSource::File { path } => path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            AssetSource::Embedded { image_index, .. } => format!("Embedded image {image_index}"),
            AssetSource::Generated { .. } => "Generated texture".into(),
        });
        let load_state = record.map_or(EnvironmentLoadState::Unset, |record| match record.state {
            TextureState::Loading => EnvironmentLoadState::Loading,
            TextureState::Ready => EnvironmentLoadState::Ready,
            TextureState::Failed => EnvironmentLoadState::Failed,
        });
        EnvironmentDetailsDto {
            scene_version: SceneVersion::from_u64(scene.scene_version()),
            enabled: sky.enabled,
            brightness: sky.brightness,
            texture_id: sky.texture.map(Self::encode_texture_id),
            file_name,
            load_state,
        }
    }

    fn edit_error(error: WorldEditError) -> EditorResponse {
        let code = if matches!(error, WorldEditError::Scene(SceneEditError::StaleHandle { .. })) {
            EditorErrorCode::StaleObject
        } else {
            EditorErrorCode::InvalidRequest
        };
        Self::error(code, error.to_string())
    }

    pub(crate) fn handle_query(
        world: &GameWorld,
        selection: Option<SceneSelection>,
        query: EditorQuery,
    ) -> EditorResponse {
        match query {
            EditorQuery::GetSceneVersion => {
                EditorResponse::SceneVersion(SceneVersion::from_u64(world.scene_view().scene_version()))
            }
            EditorQuery::GetLightDetails { light_id } => {
                Self::light_dto(world, light_id).map(EditorResponse::LightDetails).unwrap_or_else(EditorResponse::Error)
            }
            EditorQuery::GetEnvironment => EditorResponse::Environment(Self::environment_dto(world)),
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

    pub(crate) fn handle_command(
        world: &mut GameWorld,
        command: EditorCommand,
    ) -> (EditorResponse, Option<EditorNotification>) {
        let previous_version = world.scene_view().scene_version();
        match command {
            EditorCommand::UpdateLight { light_id, patch } => {
                let target = match Self::decode_light_id(&light_id) {
                    Ok(target) => target,
                    Err(e) => return (EditorResponse::Error(e), None),
                };
                let patch = truvis_world::LightPatch {
                    position: patch.position.map(Into::into),
                    radiance: patch.radiance.map(Into::into),
                    direction: patch.direction.map(Into::into),
                    inner_angle: patch.inner_angle_degrees.map(f32::to_radians),
                    outer_angle: patch.outer_angle_degrees.map(f32::to_radians),
                    rotation_degrees: patch.rotation_degrees.map(Into::into),
                    width: patch.width,
                    height: patch.height,
                };
                if let Err(e) = world.update_light(target, patch) {
                    return (Self::edit_error(e), None);
                }
                let response = Self::light_dto(world, light_id)
                    .map(EditorResponse::LightApplied)
                    .unwrap_or_else(EditorResponse::Error);
                (response, Self::version_notification(world, previous_version))
            }
            EditorCommand::UpdateEnvironment { patch } => {
                if let Err(e) = world.update_sky_parameters(patch.enabled, patch.brightness) {
                    return (Self::edit_error(e), None);
                }
                (
                    EditorResponse::EnvironmentApplied(Self::environment_dto(world)),
                    Self::version_notification(world, previous_version),
                )
            }

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

impl WorldSceneAdapter {
    fn version_notification(world: &GameWorld, previous: u64) -> Option<EditorNotification> {
        let current = world.scene_view().scene_version();
        (current != previous).then(|| EditorNotification::SceneVersionChanged(SceneVersion::from_u64(current)))
    }

    pub(crate) fn scene_objects_page(
        world: &GameWorld,
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
        let instances = view.instance_map().iter().map(|(handle, instance)| SceneObjectSummary::Instance {
            instance_id: Self::encode_instance_id(handle),
            name: instance.name.clone(),
            material_count: instance.materials.len() as u32,
        });
        let points = view.point_light_map().keys().enumerate().map(|(i, id)| SceneObjectSummary::Point {
            light_id: Self::light_identity(LightTarget::Point(id)).0,
            name: format!("Point Light {}", i + 1),
        });
        let spots = view.spot_light_map().keys().enumerate().map(|(i, id)| SceneObjectSummary::Spot {
            light_id: Self::light_identity(LightTarget::Spot(id)).0,
            name: format!("Spot Light {}", i + 1),
        });
        let areas = view.area_light_map().keys().enumerate().map(|(i, id)| SceneObjectSummary::Area {
            light_id: Self::light_identity(LightTarget::Area(id)).0,
            name: format!("Area Light {}", i + 1),
        });
        let total = view.instance_map().len() +
            view.point_light_map().len() +
            view.spot_light_map().len() +
            view.area_light_map().len() +
            1;
        let environment = SceneObjectSummary::Environment {
            name: if view.sky_state().texture.is_some() {
                "Environment / HDRI"
            } else {
                "Environment / HDRI (not set)"
            }
            .into(),
        };
        let objects = instances
            .chain(points)
            .chain(spots)
            .chain(areas)
            .chain(std::iter::once(environment))
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>();
        let consumed = offset.saturating_add(objects.len());
        let next_offset = (consumed < total).then_some(consumed as u32);

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
    pub(crate) fn instance_details(world: &GameWorld, instance_id: InstanceId) -> EditorResponse {
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

        // World 负责分解和角度约定；适配器只复制投影，分解失败不影响 mesh/material 详情。
        let transform = instance.transform_trs().ok().map(|trs| InstanceTransformDto {
            location: trs.translation.to_array(),
            rotation_degrees: trs.rotation_euler_degrees().to_array(),
            scale: trs.scale.to_array(),
        });
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

    fn selection_dto(world: &GameWorld, selection: Option<SceneSelection>) -> Option<SelectionDto> {
        match selection? {
            SceneSelection::Submesh(selection) => {
                let instance = world.scene_view().get_instance(selection.instance)?;
                let material = *instance.materials.get(selection.submesh_index as usize)?;
                Some(Self::selection_dto_from_handles(selection.instance, selection.submesh_index, material))
            }
            SceneSelection::Light(target) => {
                world.scene_view().light_position(target)?;
                Some(Self::light_selection_dto(target))
            }
        }
    }

    pub(crate) fn material_dto(world: &GameWorld, handle: MaterialAssetHandle) -> Option<MaterialDto> {
        let data = world.material_data(handle)?;
        Some(MaterialDto {
            id: Self::encode_material_id(handle),
            name: data.name.clone(),
            base_color: data.base_color.to_array(),
            metallic: data.metallic,
            roughness: data.roughness,
            class: Self::material_class_dto(data.class),
            coverage: Self::coverage_dto(data.coverage),
            textures: std::array::from_fn(|index| {
                data.textures[index].as_ref().map(|slot| TextureSlotDto {
                    texture: Self::encode_texture_id(slot.texture),
                    mapping: TextureMappingDto {
                        tex_coord: slot.tex_coord,
                        offset: slot.transform.offset.to_array(),
                        rotation: slot.transform.rotation,
                        scale: slot.transform.scale.to_array(),
                        wrap: [slot.sampler.wrap_s as u32, slot.sampler.wrap_t as u32],
                        filter: slot.sampler.filter as u32,
                    },
                })
            }),
            normal_scale: data.normal_scale,
            emissive_factor: data.emissive_factor.to_array(),
        })
    }

    pub(crate) fn apply_material_patch(
        mut data: MaterialData,
        patch: MaterialPatch,
    ) -> Result<MaterialData, EditorError> {
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
        if let Some(mappings) = patch.texture_mappings {
            for patch in mappings {
                let slot = data.textures.get_mut(patch.channel as usize).and_then(Option::as_mut).ok_or_else(|| {
                    EditorError::new(EditorErrorCode::InvalidRequest, "texture slot is absent or invalid")
                })?;
                let mapping = patch.mapping;
                let wraps = [TextureWrap::Repeat, TextureWrap::Clamp, TextureWrap::MirroredRepeat];
                let filters = [TextureFilter::Nearest, TextureFilter::Linear];
                let invalid = || EditorError::new(EditorErrorCode::InvalidRequest, "invalid texture sampler");
                slot.sampler = TextureSampler {
                    wrap_s: *wraps.get(mapping.wrap[0] as usize).ok_or_else(invalid)?,
                    wrap_t: *wraps.get(mapping.wrap[1] as usize).ok_or_else(invalid)?,
                    filter: *filters.get(mapping.filter as usize).ok_or_else(invalid)?,
                };
                slot.tex_coord = mapping.tex_coord;
                slot.transform = TextureTransform {
                    offset: mapping.offset.into(),
                    rotation: mapping.rotation,
                    scale: mapping.scale.into(),
                };
            }
        }
        if let Some(value) = patch.normal_scale {
            data.normal_scale = value;
        }
        if let Some(value) = patch.emissive_factor {
            data.emissive_factor = value.into();
        }
        data.validate().map_err(|reason| EditorError::new(EditorErrorCode::InvalidRequest, reason))?;
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

impl WorldSceneAdapter {
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

    pub(crate) fn notify_selection_changed(&self, selection: Option<SelectionChange>) {
        let selection = selection.map(|change| match change {
            SelectionChange::Submesh { selection, material } => {
                WorldSceneAdapter::selection_dto_from_handles(selection.instance, selection.submesh_index, material)
            }
            SelectionChange::Light(target) => WorldSceneAdapter::light_selection_dto(target),
        });
        let _ = self.endpoint.try_send_notification(EditorNotification::SelectionChanged(selection));
    }

    pub(crate) fn shutdown(&mut self) {
        self.endpoint.shutdown();
    }
}

impl WorldSceneAdapter {
    pub(crate) fn selection_dto_from_handles(
        instance: MeshInstanceHandle,
        submesh_index: u32,
        material: MaterialAssetHandle,
    ) -> SelectionDto {
        SelectionDto::Submesh {
            instance_id: Self::encode_instance_id(instance),
            submesh_index,
            material_id: Self::encode_material_id(material),
        }
    }

    fn encode_instance_id(handle: MeshInstanceHandle) -> InstanceId {
        InstanceId::new(Self::encode_key("instance", handle))
    }

    fn encode_material_id(handle: MaterialAssetHandle) -> MaterialId {
        MaterialId::new(Self::encode_key("material", handle))
    }

    fn encode_mesh_id(handle: MeshAssetHandle) -> MeshId {
        MeshId::new(Self::encode_key("mesh", handle))
    }

    fn encode_texture_id(handle: TextureAssetHandle) -> TextureId {
        TextureId::new(Self::encode_key("texture", handle))
    }

    pub(crate) fn encode_key<K: Key>(prefix: &str, handle: K) -> String {
        format!("{prefix}:{:016x}", handle.data().as_ffi())
    }

    pub(crate) fn decode_material_id(id: &MaterialId) -> Result<MaterialAssetHandle, EditorError> {
        Self::decode_key("material", &id.0)
    }

    pub(crate) fn decode_instance_id(id: &InstanceId) -> Result<MeshInstanceHandle, EditorError> {
        Self::decode_key("instance", &id.0)
    }

    pub(crate) fn decode_key<K: Key>(expected_prefix: &str, value: &str) -> Result<K, EditorError> {
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

    pub(crate) fn error(code: EditorErrorCode, message: impl Into<String>) -> EditorResponse {
        EditorResponse::Error(EditorError::new(code, message))
    }
}
