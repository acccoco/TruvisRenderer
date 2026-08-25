use std::collections::{HashMap, HashSet};

use truvis_world::guid_new_type::{InstanceHandle, MaterialHandle, MeshHandle, TextureHandle};
use truvis_world::{SceneChanges, SceneInstanceChangeKind, SceneReadView};

use crate::render_world::render_instance_manager::RenderInstanceUpdateResult;
use crate::render_world::render_material_manager::RenderMaterialUpdateResult;
use crate::render_world::render_mesh_manager::RenderMeshUpdateResult;
use crate::render_world::render_texture_manager::RenderTextureUpdateResult;

/// dirty routing 的阶段标签。
///
/// prepare 仍保留显式阶段边界，因为 Vulkan 资源生命周期、bindless descriptor 更新和
/// FIF buffer 写入都需要确定顺序；该枚举只负责选择每个阶段可用的静态 rule set。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirtyStageKind {
    Texture,
    AfterTexture,
    Sky,
    Material,
    AfterMaterial,
    Mesh,
    AfterMesh,
    Instance,
    AfterInstance,
    Analytic,
}

/// dirty router 接收的归一化事件。
///
/// 事件只描述“某类事实发生变化”，不直接调用 render manager，也不携带 GPU 资源 owner。
/// 规则会根据 `SceneReadView` 的只读依赖查询把事件扩展成面向具体 owner 的 dispatch plan。
#[derive(Clone, Copy, Debug)]
pub(crate) enum DirtyEvent {
    SceneTextureRemoved(TextureHandle),
    SceneMeshRemoved(MeshHandle),
    SceneMaterialChanged(MaterialHandle),
    SceneMaterialRemoved(MaterialHandle),
    SceneInstanceChanged {
        handle: InstanceHandle,
        kind: SceneInstanceChangeKind,
    },
    SceneInstanceRemoved(InstanceHandle),
    SceneSkyChanged,
    SceneAnalyticLightsChanged,
    TextureReadyChanged(TextureHandle),
    MeshReadyChanged(MeshHandle),
    MaterialSlotChanged(MaterialHandle),
    InstanceUpdated(DirtyInstanceUpdateKind),
}

/// instance manager 输出结果在 dirty routing 中的最小语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirtyInstanceUpdateKind {
    ActiveSetChanged,
    TransformChanged,
    MaterialBindingChanged,
    MeshBindingChanged,
}

/// 单条 dirty 传播规则。
///
/// 规则使用 enum + const slice 表达，避免运行时字符串配置或 trait object registry。
/// 每条规则只能读取 `SceneReadView` 的只读依赖关系，并向 `DirtyDispatchPlan` 写入数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirtyRuleKind {
    SceneTextureRemovedMarksTextureRemoved,
    TextureReadyChangedMarksDependentMaterials,
    TextureReadyChangedMarksSky,
    SceneSkyChangedMarksSky,
    SceneMaterialChangedMarksMaterial,
    SceneMaterialChangedMarksDependentInstances,
    SceneMaterialRemovedMarksMaterialRemoved,
    MaterialChangedMarksEmissive,
    MaterialSlotChangedMarksDependentInstances,
    MaterialSlotChangedMarksEmissive,
    SceneMeshRemovedMarksMeshRemoved,
    MeshReadyChangedMarksDependentInstances,
    MeshReadyChangedMarksEmissive,
    SceneInstanceChangedMarksInstance,
    SceneInstanceRemovedMarksInstanceRemoved,
    InstanceUpdateMarksTlas,
    InstanceUpdateMarksEmissive,
    SceneAnalyticLightsChangedMarksAnalytic,
}

pub(crate) const TEXTURE_STAGE_RULES: &[DirtyRuleKind] = &[DirtyRuleKind::SceneTextureRemovedMarksTextureRemoved];

pub(crate) const AFTER_TEXTURE_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::TextureReadyChangedMarksDependentMaterials,
    DirtyRuleKind::TextureReadyChangedMarksSky,
];

pub(crate) const SKY_STAGE_RULES: &[DirtyRuleKind] = &[DirtyRuleKind::SceneSkyChangedMarksSky];

pub(crate) const MATERIAL_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::SceneMaterialChangedMarksMaterial,
    DirtyRuleKind::SceneMaterialChangedMarksDependentInstances,
    DirtyRuleKind::SceneMaterialRemovedMarksMaterialRemoved,
    DirtyRuleKind::MaterialChangedMarksEmissive,
];

pub(crate) const AFTER_MATERIAL_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::MaterialSlotChangedMarksDependentInstances,
    DirtyRuleKind::MaterialSlotChangedMarksEmissive,
];

pub(crate) const MESH_STAGE_RULES: &[DirtyRuleKind] = &[DirtyRuleKind::SceneMeshRemovedMarksMeshRemoved];

pub(crate) const AFTER_MESH_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::MeshReadyChangedMarksDependentInstances,
    DirtyRuleKind::MeshReadyChangedMarksEmissive,
];

pub(crate) const INSTANCE_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::SceneInstanceChangedMarksInstance,
    DirtyRuleKind::SceneInstanceRemovedMarksInstanceRemoved,
];

pub(crate) const AFTER_INSTANCE_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::InstanceUpdateMarksTlas,
    DirtyRuleKind::InstanceUpdateMarksEmissive,
];

pub(crate) const ANALYTIC_STAGE_RULES: &[DirtyRuleKind] = &[DirtyRuleKind::SceneAnalyticLightsChangedMarksAnalytic];

/// material dispatch 的 dirty 原因。
///
/// CPU material 参数变化会影响自发光估算和 manager 内部 material revision；texture ready
/// 只需要把 fallback binding 刷成真实 SRV，不应伪装成 CPU 语义变化。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DirtyMaterialFlags {
    pub(crate) scene_changed: bool,
    pub(crate) texture_ready_changed: bool,
}

impl DirtyMaterialFlags {
    pub(crate) fn scene_changed() -> Self {
        Self {
            scene_changed: true,
            texture_ready_changed: false,
        }
    }

    pub(crate) fn texture_ready_changed() -> Self {
        Self {
            scene_changed: false,
            texture_ready_changed: true,
        }
    }

    fn merge(&mut self, other: Self) {
        self.scene_changed |= other.scene_changed;
        self.texture_ready_changed |= other.texture_ready_changed;
    }
}

/// instance dispatch 的 dirty 原因。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DirtyInstanceFlags {
    pub(crate) lifecycle: bool,
    pub(crate) transform: bool,
    pub(crate) material_binding: bool,
    pub(crate) mesh_binding: bool,
}

impl DirtyInstanceFlags {
    pub(crate) fn from_scene_kind(kind: SceneInstanceChangeKind) -> Self {
        match kind {
            SceneInstanceChangeKind::Lifecycle => Self {
                lifecycle: true,
                transform: true,
                material_binding: true,
                mesh_binding: true,
            },
            SceneInstanceChangeKind::MaterialBinding => Self {
                material_binding: true,
                ..Self::default()
            },
            SceneInstanceChangeKind::Transform => Self {
                transform: true,
                ..Self::default()
            },
        }
    }

    pub(crate) fn material_binding() -> Self {
        Self {
            material_binding: true,
            ..Self::default()
        }
    }

    pub(crate) fn mesh_binding() -> Self {
        Self {
            mesh_binding: true,
            ..Self::default()
        }
    }

    pub(crate) fn needs_ready_check(self) -> bool {
        self.lifecycle || self.material_binding || self.mesh_binding
    }

    fn merge(&mut self, other: Self) {
        self.lifecycle |= other.lifecycle;
        self.transform |= other.transform;
        self.material_binding |= other.material_binding;
        self.mesh_binding |= other.mesh_binding;
    }
}

/// material manager 本帧需要消费的 dispatch 数据。
#[derive(Default)]
pub(crate) struct MaterialDispatch {
    pub(crate) dirty_materials: HashMap<MaterialHandle, DirtyMaterialFlags>,
    pub(crate) removed_materials: Vec<MaterialHandle>,
}

/// instance manager 本帧需要消费的 dispatch 数据。
#[derive(Default)]
pub(crate) struct InstanceDispatch {
    pub(crate) dirty_instances: HashMap<InstanceHandle, DirtyInstanceFlags>,
    pub(crate) removed_instances: Vec<InstanceHandle>,
}

/// 规则路由后的本帧分发计划。
///
/// 它不是 GPU command buffer：这里只保存各 owner 在 prepare 阶段需要消费的 CPU handle
/// 和 dirty 标志。plan 的生命周期限制在本次 `RenderRuntime::prepare_render_world` 内，
/// 不拥有 Vulkan 资源，也不会跨帧保存。
#[derive(Default)]
pub(crate) struct DirtyDispatchPlan {
    texture_removals: HashSet<TextureHandle>,
    mesh_removals: HashSet<MeshHandle>,
    material_dirty: HashMap<MaterialHandle, DirtyMaterialFlags>,
    material_removed: HashSet<MaterialHandle>,
    instance_dirty: HashMap<InstanceHandle, DirtyInstanceFlags>,
    instance_removed: HashSet<InstanceHandle>,
    sky_dirty: bool,
    analytic_dirty: bool,
    emissive_dirty: bool,
    tlas_dirty: bool,
}

impl DirtyDispatchPlan {
    pub(crate) fn mark_texture_removed(&mut self, handle: TextureHandle) {
        self.texture_removals.insert(handle);
    }

    pub(crate) fn mark_mesh_removed(&mut self, handle: MeshHandle) {
        self.mesh_removals.insert(handle);
    }

    pub(crate) fn mark_material_dirty(&mut self, handle: MaterialHandle, flags: DirtyMaterialFlags) {
        if self.material_removed.contains(&handle) {
            return;
        }
        self.material_dirty.entry(handle).or_default().merge(flags);
    }

    pub(crate) fn mark_material_removed(&mut self, handle: MaterialHandle) {
        self.material_dirty.remove(&handle);
        self.material_removed.insert(handle);
    }

    pub(crate) fn mark_instance_dirty(&mut self, handle: InstanceHandle, flags: DirtyInstanceFlags) {
        if self.instance_removed.contains(&handle) {
            return;
        }
        self.instance_dirty.entry(handle).or_default().merge(flags);
    }

    pub(crate) fn mark_instance_removed(&mut self, handle: InstanceHandle) {
        self.instance_dirty.remove(&handle);
        self.instance_removed.insert(handle);
    }

    pub(crate) fn mark_sky_dirty(&mut self) {
        self.sky_dirty = true;
    }

    pub(crate) fn mark_analytic_dirty(&mut self) {
        self.analytic_dirty = true;
    }

    pub(crate) fn mark_emissive_dirty(&mut self) {
        self.emissive_dirty = true;
    }

    pub(crate) fn mark_tlas_dirty(&mut self) {
        self.tlas_dirty = true;
    }

    pub(crate) fn take_texture_removals(&mut self) -> Vec<TextureHandle> {
        self.texture_removals.drain().collect()
    }

    pub(crate) fn take_mesh_removals(&mut self) -> Vec<MeshHandle> {
        self.mesh_removals.drain().collect()
    }

    pub(crate) fn take_material_dispatch(&mut self) -> MaterialDispatch {
        MaterialDispatch {
            dirty_materials: std::mem::take(&mut self.material_dirty),
            removed_materials: self.material_removed.drain().collect(),
        }
    }

    pub(crate) fn take_instance_dispatch(&mut self) -> InstanceDispatch {
        InstanceDispatch {
            dirty_instances: std::mem::take(&mut self.instance_dirty),
            removed_instances: self.instance_removed.drain().collect(),
        }
    }

    pub(crate) fn take_sky_dirty(&mut self) -> bool {
        std::mem::take(&mut self.sky_dirty)
    }

    pub(crate) fn take_analytic_dirty(&mut self) -> bool {
        std::mem::take(&mut self.analytic_dirty)
    }

    pub(crate) fn take_emissive_dirty(&mut self) -> bool {
        std::mem::take(&mut self.emissive_dirty)
    }

    pub(crate) fn take_tlas_dirty(&mut self) -> bool {
        std::mem::take(&mut self.tlas_dirty)
    }
}

/// dirty 事件到 dispatch plan 的转换 helper。
pub(crate) struct DirtyRouterHelper;

impl DirtyRouterHelper {
    pub(crate) fn events_from_scene_changes(changes: &SceneChanges) -> Vec<DirtyEvent> {
        let mut events = Vec::new();
        events.extend(changes.removed_textures.iter().copied().map(DirtyEvent::SceneTextureRemoved));
        events.extend(changes.removed_meshes.iter().copied().map(DirtyEvent::SceneMeshRemoved));
        events.extend(changes.changed_materials.iter().copied().map(DirtyEvent::SceneMaterialChanged));
        events.extend(changes.removed_materials.iter().copied().map(DirtyEvent::SceneMaterialRemoved));
        events.extend(changes.changed_instances.iter().map(|change| DirtyEvent::SceneInstanceChanged {
            handle: change.handle,
            kind: change.kind,
        }));
        events.extend(changes.removed_instances.iter().copied().map(DirtyEvent::SceneInstanceRemoved));
        if changes.changed_sky_environment {
            events.push(DirtyEvent::SceneSkyChanged);
        }
        if changes.changed_analytic_lights {
            events.push(DirtyEvent::SceneAnalyticLightsChanged);
        }
        events
    }

    pub(crate) fn events_from_texture_update_result(result: RenderTextureUpdateResult) -> Vec<DirtyEvent> {
        result.ready_changed_textures.into_iter().map(DirtyEvent::TextureReadyChanged).collect()
    }

    pub(crate) fn events_from_mesh_update_result(result: RenderMeshUpdateResult) -> Vec<DirtyEvent> {
        result.ready_changed_meshes.into_iter().map(DirtyEvent::MeshReadyChanged).collect()
    }

    pub(crate) fn events_from_material_update_result(result: RenderMaterialUpdateResult) -> Vec<DirtyEvent> {
        result.slot_changed_materials.into_iter().map(DirtyEvent::MaterialSlotChanged).collect()
    }

    pub(crate) fn events_from_instance_update_result(result: RenderInstanceUpdateResult) -> Vec<DirtyEvent> {
        let mut events = Vec::new();
        if result.active_set_changed {
            events.push(DirtyEvent::InstanceUpdated(DirtyInstanceUpdateKind::ActiveSetChanged));
        }
        if result.transform_changed {
            events.push(DirtyEvent::InstanceUpdated(DirtyInstanceUpdateKind::TransformChanged));
        }
        if result.material_binding_changed {
            events.push(DirtyEvent::InstanceUpdated(DirtyInstanceUpdateKind::MaterialBindingChanged));
        }
        if result.mesh_binding_changed {
            events.push(DirtyEvent::InstanceUpdated(DirtyInstanceUpdateKind::MeshBindingChanged));
        }
        events
    }

    pub(crate) fn route_stage(
        stage: DirtyStageKind,
        events: &[DirtyEvent],
        scene: SceneReadView<'_>,
        out: &mut DirtyDispatchPlan,
    ) {
        let rules = match stage {
            DirtyStageKind::Texture => TEXTURE_STAGE_RULES,
            DirtyStageKind::AfterTexture => AFTER_TEXTURE_STAGE_RULES,
            DirtyStageKind::Sky => SKY_STAGE_RULES,
            DirtyStageKind::Material => MATERIAL_STAGE_RULES,
            DirtyStageKind::AfterMaterial => AFTER_MATERIAL_STAGE_RULES,
            DirtyStageKind::Mesh => MESH_STAGE_RULES,
            DirtyStageKind::AfterMesh => AFTER_MESH_STAGE_RULES,
            DirtyStageKind::Instance => INSTANCE_STAGE_RULES,
            DirtyStageKind::AfterInstance => AFTER_INSTANCE_STAGE_RULES,
            DirtyStageKind::Analytic => ANALYTIC_STAGE_RULES,
        };
        Self::route_events(rules, events, scene, out);
    }

    pub(crate) fn route_events(
        rules: &[DirtyRuleKind],
        events: &[DirtyEvent],
        scene: SceneReadView<'_>,
        out: &mut DirtyDispatchPlan,
    ) {
        for &rule in rules {
            for &event in events {
                Self::route_rule(rule, event, scene, out);
            }
        }
    }

    fn route_rule(rule: DirtyRuleKind, event: DirtyEvent, scene: SceneReadView<'_>, out: &mut DirtyDispatchPlan) {
        match (rule, event) {
            (DirtyRuleKind::SceneTextureRemovedMarksTextureRemoved, DirtyEvent::SceneTextureRemoved(texture)) => {
                out.mark_texture_removed(texture);
            }
            (DirtyRuleKind::TextureReadyChangedMarksDependentMaterials, DirtyEvent::TextureReadyChanged(texture)) => {
                for material in scene.materials_using_texture(texture) {
                    out.mark_material_dirty(material, DirtyMaterialFlags::texture_ready_changed());
                }
            }
            (DirtyRuleKind::TextureReadyChangedMarksSky, DirtyEvent::TextureReadyChanged(texture)) => {
                if scene.sky_uses_texture(texture) {
                    out.mark_sky_dirty();
                }
            }
            (DirtyRuleKind::SceneSkyChangedMarksSky, DirtyEvent::SceneSkyChanged) => {
                out.mark_sky_dirty();
            }
            (DirtyRuleKind::SceneMaterialChangedMarksMaterial, DirtyEvent::SceneMaterialChanged(material)) => {
                out.mark_material_dirty(material, DirtyMaterialFlags::scene_changed());
            }
            (
                DirtyRuleKind::SceneMaterialChangedMarksDependentInstances,
                DirtyEvent::SceneMaterialChanged(material),
            ) => {
                // coverage / alpha cutoff 会影响 active instance 是否需要 any-hit；
                // v1 保守地让依赖该 material 的实例重算 binding，并由 instance update 推进 TLAS。
                for instance in scene.instances_using_material(material) {
                    out.mark_instance_dirty(instance, DirtyInstanceFlags::material_binding());
                }
            }
            (DirtyRuleKind::SceneMaterialRemovedMarksMaterialRemoved, DirtyEvent::SceneMaterialRemoved(material)) => {
                out.mark_material_removed(material);
            }
            (
                DirtyRuleKind::MaterialChangedMarksEmissive,
                DirtyEvent::SceneMaterialChanged(_) | DirtyEvent::SceneMaterialRemoved(_),
            ) => {
                out.mark_emissive_dirty();
            }
            (DirtyRuleKind::MaterialSlotChangedMarksDependentInstances, DirtyEvent::MaterialSlotChanged(material)) => {
                for instance in scene.instances_using_material(material) {
                    out.mark_instance_dirty(instance, DirtyInstanceFlags::material_binding());
                }
            }
            (DirtyRuleKind::MaterialSlotChangedMarksEmissive, DirtyEvent::MaterialSlotChanged(_)) => {
                out.mark_emissive_dirty();
            }
            (DirtyRuleKind::SceneMeshRemovedMarksMeshRemoved, DirtyEvent::SceneMeshRemoved(mesh)) => {
                out.mark_mesh_removed(mesh);
            }
            (DirtyRuleKind::MeshReadyChangedMarksDependentInstances, DirtyEvent::MeshReadyChanged(mesh)) => {
                for instance in scene.instances_using_mesh(mesh) {
                    out.mark_instance_dirty(instance, DirtyInstanceFlags::mesh_binding());
                }
            }
            (DirtyRuleKind::MeshReadyChangedMarksEmissive, DirtyEvent::MeshReadyChanged(_)) => {
                out.mark_emissive_dirty();
            }
            (DirtyRuleKind::SceneInstanceChangedMarksInstance, DirtyEvent::SceneInstanceChanged { handle, kind }) => {
                out.mark_instance_dirty(handle, DirtyInstanceFlags::from_scene_kind(kind));
            }
            (DirtyRuleKind::SceneInstanceRemovedMarksInstanceRemoved, DirtyEvent::SceneInstanceRemoved(instance)) => {
                out.mark_instance_removed(instance);
            }
            (DirtyRuleKind::InstanceUpdateMarksTlas, DirtyEvent::InstanceUpdated(kind)) => match kind {
                DirtyInstanceUpdateKind::ActiveSetChanged
                | DirtyInstanceUpdateKind::TransformChanged
                | DirtyInstanceUpdateKind::MaterialBindingChanged
                | DirtyInstanceUpdateKind::MeshBindingChanged => out.mark_tlas_dirty(),
            },
            (DirtyRuleKind::InstanceUpdateMarksEmissive, DirtyEvent::InstanceUpdated(_)) => {
                out.mark_emissive_dirty();
            }
            (DirtyRuleKind::SceneAnalyticLightsChangedMarksAnalytic, DirtyEvent::SceneAnalyticLightsChanged) => {
                out.mark_analytic_dirty();
            }
            _ => {}
        }
    }
}
