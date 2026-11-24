use indexmap::IndexSet;
use slotmap::{SecondaryMap, SlotMap};
use truvis_asset::handle::{MaterialAssetHandle, TextureAssetHandle};
use truvis_shader_binding::gpu;

use crate::{
    AreaLightShape, LightPatch, LightTarget,
    components::instance::Instance,
    edit_error::{SceneEditError, SceneHandleKind},
    guid_new_type::{LightHandle, MeshInstanceHandle},
};

/// CPU scene 中的 sky / environment 权威状态。
///
/// 这里仅保存 Renderer 可编辑的语义状态：是否启用、亮度、引用的 scene texture 以及语义版本。
/// GPU SRV、fallback texture、importance distribution 和 retired buffer 都属于 render-side
/// `GpuSkyStore`，不会进入 `GameWorld`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneSkyState {
    pub enabled: bool,
    pub brightness: f32,
    pub texture: Option<TextureAssetHandle>,
    pub revision: u64,
}

impl Default for SceneSkyState {
    fn default() -> Self {
        Self {
            enabled: true,
            brightness: 1.0,
            texture: None,
            revision: 0,
        }
    }
}

impl GameWorld {
    pub fn new() -> Self {
        Self::default()
    }

    /// 更新 CPU sky 是否启用。
    pub fn update_sky_enabled(&mut self, enabled: bool) {
        self.update_sky_parameters(Some(enabled), None).expect("unchanged brightness is valid");
    }

    /// 修改灯光世界位置，保留灯光形状与强度。
    pub fn update_light_position(&mut self, target: LightTarget, position: glam::Vec3) -> Result<(), SceneEditError> {
        self.update_light(target, LightPatch { position: Some(position), ..Default::default() })
    }

    /// 返回全部 live instance。
    ///
    /// 调用方不应把 map key 理解为 GPU slot；稳定 slot 由 render-side manager 独立维护。
    #[inline]
    pub fn instance_map(&self) -> &SlotMap<MeshInstanceHandle, Instance> {
        &self.all_instances
    }

    /// 返回 CPU scene 的单调递增全局语义版本。
    ///
    /// 该版本不会在 prepare 时清零，也不表示 GPU prepare 已完成；Editor
    /// 只用它检测多次最终一致查询之间是否发生 CPU 语义变化。
    #[inline]
    pub fn scene_version(&self) -> u64 {
        self.scene_version
    }

    /// 按带类型身份读取世界位置，不创建 transform 副本。
    pub fn light_position(&self, target: LightTarget) -> Option<glam::Vec3> {
        match target {
            LightTarget::Point(handle) => self.all_point_lights.get(handle).map(|light| light.pos.into()),
            LightTarget::Spot(handle) => self.all_spot_lights.get(handle).map(|light| light.pos.into()),
            LightTarget::Area(handle) => self.all_area_lights.get(handle).map(|light| light.center.into()),
        }
    }

    /// 返回全部 live point light。
    #[inline]
    pub fn point_light_map(&self) -> &SlotMap<LightHandle, gpu::engine::light::PointLight> {
        &self.all_point_lights
    }

    /// 返回全部 live spot light。
    #[inline]
    pub fn spot_light_map(&self) -> &SlotMap<LightHandle, gpu::engine::light::SpotLight> {
        &self.all_spot_lights
    }

    /// 返回全部 live area light。
    #[inline]
    pub fn area_light_map(&self) -> &SlotMap<LightHandle, gpu::engine::light::AreaLight> {
        &self.all_area_lights
    }

    /// 返回 analytic light 语义版本；只表达 point/spot/area 光源快照变化。
    #[inline]
    pub fn light_revision(&self) -> u32 {
        self.light_revision
    }

    /// 返回 sky / environment 的 CPU 权威状态。
    #[inline]
    pub fn sky_state(&self) -> &SceneSkyState {
        &self.sky_state
    }

    /// 按 CPU runtime handle 查询 live instance。
    #[inline]
    pub fn get_instance(&self, handle: MeshInstanceHandle) -> Option<&Instance> {
        self.all_instances.get(handle)
    }

    /// 返回 instance 最近一次有效编辑的 source revision。
    #[inline]
    pub fn instance_revision(&self, handle: MeshInstanceHandle) -> Option<u64> {
        self.instance_revisions.get(handle).copied()
    }
}

/// 当前 World 的 CPU 场景权威。
///
/// 实例、灯光和环境在 update 阶段提交。资产 handle 在场景中只表示引用。
#[derive(Default)]
pub struct GameWorld {
    /// CPU scene 的全局语义版本；只在实际 mutation 成功后推进，失败和 no-op 不推进。
    scene_version: u64,
    /// live instance 存储；slotmap key 是 CPU scene 内部的 runtime 身份。
    all_instances: SlotMap<MeshInstanceHandle, Instance>,
    /// 每个 instance 的 source revision；只在有效 transform/material 编辑后推进。
    instance_revisions: SecondaryMap<MeshInstanceHandle, u64>,
    /// 只记录待对账身份，包含已删除 handle；紧凑存储使消费成本与变更数相关。
    changed_instances: IndexSet<MeshInstanceHandle>,
    /// CPU sky / environment 权威状态。
    sky_state: SceneSkyState,
    /// live point light 存储；GPU 侧打包和上传由 render runtime 处理。
    all_point_lights: SlotMap<LightHandle, gpu::engine::light::PointLight>,
    /// live spot light 存储；与 point light 分开保存，避免 CPU 语义层提前引入统一 light class。
    all_spot_lights: SlotMap<LightHandle, gpu::engine::light::SpotLight>,
    /// live area light 存储；矩形单面发光的采样语义由 realtime RT shader 解释。
    all_area_lights: SlotMap<LightHandle, gpu::engine::light::AreaLight>,
    /// point/spot/area light 语义变化版本，用于渲染端拒绝不匹配的 ReSTIR history。
    light_revision: u32,
}
// 创建与初始化
impl GameWorld {
    /// prepare 同步接管整个批次；CPU 最终状态仍由 GameWorld 持有。
    pub fn take_instance_changes(&mut self) -> IndexSet<MeshInstanceHandle> {
        std::mem::take(&mut self.changed_instances)
    }

    fn bump_scene_version(&mut self) {
        // u64 饱和在实际工程生命周期内不可达；使用饱和加法保持“不会回退”的协议契约。
        self.scene_version = self.scene_version.saturating_add(1);
    }

}
// 访问器
impl GameWorld {
    /// 0 区分默认空状态；饱和后不回退，但容量耗尽时无法继续区分编辑。
    fn bump_light_revision(&mut self) {
        self.light_revision = self.light_revision.saturating_add(1).max(1);
    }

    fn bump_sky_revision(&mut self) {
        // sky revision 只表达 CPU 环境光语义变化；GPU distribution 版本仍由 GpuSkyStore
        // 独立维护，并在 scene root 中单独写入。
        self.sky_state.revision = self.sky_state.revision.saturating_add(1).max(1);
    }

    /// 更新 CPU sky 引用的 scene texture。
    pub fn update_sky_texture(&mut self, texture: Option<TextureAssetHandle>) {
        if self.sky_state.texture != texture {
            self.sky_state.texture = texture;
            self.bump_sky_revision();
            self.bump_scene_version();
        }
    }

    pub fn update_sky_parameters(
        &mut self,
        enabled: Option<bool>,
        brightness: Option<f32>,
    ) -> Result<(), SceneEditError> {
        let brightness = brightness.unwrap_or(self.sky_state.brightness);
        if !brightness.is_finite() || brightness < 0.0 {
            return Err(SceneEditError::InvalidSkyData {
                reason: "brightness must be finite and nonnegative".into(),
            });
        }
        let enabled = enabled.unwrap_or(self.sky_state.enabled);
        if enabled != self.sky_state.enabled || brightness != self.sky_state.brightness {
            self.sky_state.enabled = enabled;
            self.sky_state.brightness = brightness;
            self.bump_sky_revision();
            self.bump_scene_version();
        }
        Ok(())
    }

    /// 调用方提供已解析的资产引用和 mesh submesh 数量，World 校验实例自己的绑定形状。
    pub fn create_mesh_instance(&mut self, instance: Instance, submesh_count: usize) -> Result<MeshInstanceHandle, SceneEditError> {
        instance.validate_material_count(submesh_count)?;
        Instance::validate_transform(instance.transform).map_err(|reason| SceneEditError::InvalidInstanceData { reason })?;
        let handle = self.all_instances.insert(instance);
        self.instance_revisions.insert(handle, 1);
        self.changed_instances.insert(handle);
        self.bump_scene_version();
        Ok(handle)
    }

    /// 从 CPU scene 移除 live instance。
    ///
    /// 返回的 instance 数据只代表 CPU 记录。已建立的 GPU-side 映射会在后续 prepare/sync
    /// 阶段被 `RenderInstanceTable` 识别为 stale 并回收。
    pub fn remove_mesh_instance(&mut self, handle: MeshInstanceHandle) -> Result<(), SceneEditError> {
        if self.all_instances.remove(handle).is_none() {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Instance,
            });
        };
        self.instance_revisions.remove(handle);
        self.changed_instances.insert(handle);
        self.bump_scene_version();
        Ok(())
    }

    /// 更新 live instance 的 CPU world transform。
    ///
    /// 失效 handle 或非法矩阵返回错误，失败与 no-op 不推进版本或记录变更身份。
    /// GPU scene 数据由下一次 render runtime 同步更新。
    pub fn update_instance_transform(
        &mut self,
        handle: MeshInstanceHandle,
        transform: glam::Mat4,
    ) -> Result<(), SceneEditError> {
        let Some(instance) = self.all_instances.get_mut(handle) else {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Instance,
            });
        };
        Instance::validate_transform(transform).map_err(|reason| SceneEditError::InvalidInstanceData { reason })?;
        if instance.transform == transform {
            return Ok(());
        }
        instance.transform = transform;
        self.changed_instances.insert(handle);
        let revision = self.instance_revisions.get_mut(handle).expect("GameWorld: instance revision missing");
        *revision = revision.saturating_add(1).max(1);
        self.bump_scene_version();
        Ok(())
    }

    /// 更新 instance 的 material 列表。
    ///
    /// v1 不允许更新 instance mesh 引用，因此这里只维护 material -> instance 反向依赖；
    /// mesh 反向依赖保持创建时的关系不变。
    pub fn update_instance_materials(
        &mut self,
        handle: MeshInstanceHandle,
        materials: Vec<MaterialAssetHandle>,
    ) -> Result<(), SceneEditError> {
        let Some(old_instance) = self.all_instances.get(handle).cloned() else {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Instance,
            });
        };
        let expected = old_instance.materials.len();
        if materials.len() != expected {
            return Err(SceneEditError::MaterialCountMismatch { expected, actual: materials.len() });
        }
        if old_instance.materials == materials {
            return Ok(());
        }

        self.all_instances[handle].materials = materials;
        self.changed_instances.insert(handle);
        let revision = self.instance_revisions.get_mut(handle).expect("GameWorld: instance revision missing");
        *revision = revision.saturating_add(1).max(1);
        self.bump_scene_version();
        Ok(())
    }

    /// 候选参数完整校验后一次提交；Gizmo 和 Inspector 共用版本推进，不覆盖未编辑字段。
    pub fn update_light(&mut self, target: LightTarget, patch: LightPatch) -> Result<(), SceneEditError> {
        let stale = SceneEditError::StaleHandle {
            kind: SceneHandleKind::Light,
        };
        let changed = match target {
            LightTarget::Point(handle) => {
                let light = self.all_point_lights.get_mut(handle).ok_or(stale)?;
                if patch.has_spot_fields() || patch.has_area_fields() {
                    return Err(LightPatch::invalid("unsupported point light field"));
                }
                let position = patch.position.unwrap_or(light.pos.into());
                let radiance = patch.radiance.unwrap_or(light.color.into());
                LightPatch::validate_common(position, radiance)?;
                let changed = position != light.pos.into() || radiance != light.color.into();
                light.pos = position.into();
                light.color = radiance.into();
                changed
            }
            LightTarget::Spot(handle) => {
                let light = self.all_spot_lights.get_mut(handle).ok_or(stale)?;
                if patch.has_area_fields() {
                    return Err(LightPatch::invalid("unsupported spot light field"));
                }
                let position = patch.position.unwrap_or(light.pos.into());
                let radiance = patch.radiance.unwrap_or(light.color.into());
                LightPatch::validate_common(position, radiance)?;
                let direction = patch.direction.unwrap_or(light.dir.into());
                let inner = patch.inner_angle.unwrap_or(light.inner_angle);
                let outer = patch.outer_angle.unwrap_or(light.outer_angle);
                let direction = LightPatch::validate_spot(direction, inner, outer)?;
                let changed = position != light.pos.into() ||
                    radiance != light.color.into() ||
                    direction != light.dir.into() ||
                    inner != light.inner_angle ||
                    outer != light.outer_angle;
                light.pos = position.into();
                light.color = radiance.into();
                light.dir = direction.into();
                light.inner_angle = inner;
                light.outer_angle = outer;
                changed
            }
            LightTarget::Area(handle) => {
                let light = self.all_area_lights.get_mut(handle).ok_or(stale)?;
                if patch.has_spot_fields() {
                    return Err(LightPatch::invalid("unsupported area light field"));
                }
                let position = patch.position.unwrap_or(light.center.into());
                let radiance = patch.radiance.unwrap_or(light.radiance.into());
                LightPatch::validate_common(position, radiance)?;
                let (u, v) = if patch.has_area_fields() {
                    let mut shape =
                        AreaLightShape::from_axes(light.half_u.into(), light.half_v.into()).ok_or_else(|| {
                            LightPatch::invalid("area axes cannot be represented as rotation and dimensions")
                        })?;
                    if shape.matches_patch(patch) {
                        (light.half_u.into(), light.half_v.into())
                    } else {
                        shape.apply(patch)?
                    }
                } else {
                    (light.half_u.into(), light.half_v.into())
                };
                let changed = position != light.center.into() ||
                    radiance != light.radiance.into() ||
                    u != light.half_u.into() ||
                    v != light.half_v.into();
                light.center = position.into();
                light.radiance = radiance.into();
                light.half_u = u.into();
                light.half_v = v.into();
                changed
            }
        };
        if changed {
            self.bump_light_revision();
            self.bump_scene_version();
        }
        Ok(())
    }

    /// 向 CPU scene 添加一个 live point light。
    ///
    /// 光源使用 shader binding 中的共享布局类型，但这里仍只负责 CPU 侧生命周期；GPU buffer
    /// 更新由 render runtime 的 scene 同步流程处理。
    pub fn register_point_light(&mut self, light: gpu::engine::light::PointLight) -> Result<LightHandle, SceneEditError> {
        LightPatch::validate_common(light.pos.into(), light.color.into())?;
        let handle = self.all_point_lights.insert(light);
        self.bump_light_revision();
        self.bump_scene_version();
        Ok(handle)
    }

    /// 向 CPU scene 添加一个 live spot light。
    ///
    /// spot light 在 realtime RT 中表示半径固定为 0.005 m 的 sphere emitter，并额外带 cone
    /// falloff；角度单位为 radians，注册与编辑共用方向归一化和锥角校验。
    pub fn register_spot_light(&mut self, mut light: gpu::engine::light::SpotLight) -> Result<LightHandle, SceneEditError> {
        LightPatch::validate_common(light.pos.into(), light.color.into())?;
        light.dir = LightPatch::validate_spot(light.dir.into(), light.inner_angle, light.outer_angle)?.into();
        let handle = self.all_spot_lights.insert(light);
        self.bump_light_revision();
        self.bump_scene_version();
        Ok(handle)
    }

    /// 向 CPU scene 添加一个 live area light。
    ///
    /// area light 使用 world-space 半轴描述矩形；复用编辑投影检查其可表示性，保留原轴。
    /// 这里不缓存法线或面积，几何派生仍归 render-side/shader 所有。
    pub fn register_area_light(&mut self, light: gpu::engine::light::AreaLight) -> Result<LightHandle, SceneEditError> {
        LightPatch::validate_common(light.center.into(), light.radiance.into())?;
        AreaLightShape::from_axes(light.half_u.into(), light.half_v.into())
            .ok_or_else(|| LightPatch::invalid("area axes must describe a finite nondegenerate rectangle"))?;
        let handle = self.all_area_lights.insert(light);
        self.bump_light_revision();
        self.bump_scene_version();
        Ok(handle)
    }
}

// 场景关系索引与 edit 校验
#[cfg(test)]
mod tests {
    use slotmap::KeyData;

    use truvis_asset::handle::MeshAssetHandle;

    use super::*;

    #[test]
    fn validates_instance_material_count_against_mesh_submeshes() {
        let mut scene = GameWorld::new();
        let mesh = MeshAssetHandle::from(KeyData::from_ffi(1));
        let material_a = MaterialAssetHandle::from(KeyData::from_ffi(1));
        let material_b = MaterialAssetHandle::from(KeyData::from_ffi(2));

        let err = scene
            .create_mesh_instance(Instance {
                name: "invalid-material-count".to_string(),
                mesh,
                materials: vec![material_a],
                transform: glam::Mat4::IDENTITY,
            }, 2)
            .unwrap_err();
        assert_eq!(err, SceneEditError::MaterialCountMismatch { expected: 2, actual: 1 });

        let instance = scene
            .create_mesh_instance(Instance {
                name: "valid-material-count".to_string(),
                mesh,
                materials: vec![material_a, material_b],
                transform: glam::Mat4::IDENTITY,
            }, 2)
            .unwrap();

        let err = scene.update_instance_materials(instance, vec![material_a]).unwrap_err();
        assert_eq!(err, SceneEditError::MaterialCountMismatch { expected: 2, actual: 1 });
    }
}
