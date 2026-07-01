use std::collections::{HashMap, HashSet};

use slotmap::SlotMap;

use truvis_shader_binding::gpu;

use crate::components::instance::Instance;
use crate::components::material::MaterialData;
use crate::edit_error::{SceneEditError, SceneHandleKind};
use crate::guid_new_type::{InstanceHandle, LightHandle, MaterialHandle, MeshHandle, TextureHandle};

/// CPU scene 中 instance 语义变化的强度。
///
/// 该枚举只描述 CPU 侧 edit 语义，不表示 GPU buffer dirty。render-side dirty routing 会在
/// prepare 阶段把它转换成 instance buffer、material binding、TLAS 或 emissive table 的具体 dirty。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneInstanceChangeKind {
    /// instance 新增、删除前的生命周期变化，强度最高。
    Lifecycle,
    /// instance 的 material 列表发生变化；v1 不支持更新 mesh 引用。
    MaterialBinding,
    /// instance 的 world transform 发生变化。
    Transform,
}

impl SceneInstanceChangeKind {
    fn merge(self, other: Self) -> Self {
        use SceneInstanceChangeKind::{Lifecycle, MaterialBinding, Transform};
        match (self, other) {
            (Lifecycle, _) | (_, Lifecycle) => Lifecycle,
            (MaterialBinding, _) | (_, MaterialBinding) => MaterialBinding,
            (Transform, Transform) => Transform,
        }
    }
}

/// drain 后输出给 render prepare 的单个 instance change。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneInstanceChange {
    pub handle: InstanceHandle,
    pub kind: SceneInstanceChangeKind,
}

/// `World::sync_for_render()` 输出的 CPU scene 语义变化。
///
/// texture / mesh 添加不进入这里，而是通过短期 pending upload payload 进入 render side。
/// 本结构只表达 CPU 语义变化，不表达 GPU ready、upload dirty 或资源释放状态。
#[derive(Debug, Default)]
pub struct SceneChanges {
    pub removed_textures: Vec<TextureHandle>,
    pub removed_meshes: Vec<MeshHandle>,
    pub changed_materials: Vec<MaterialHandle>,
    pub removed_materials: Vec<MaterialHandle>,
    pub changed_instances: Vec<SceneInstanceChange>,
    pub removed_instances: Vec<InstanceHandle>,
    pub changed_sky_environment: bool,
    pub changed_analytic_lights: bool,
}

impl SceneChanges {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.removed_textures.is_empty()
            && self.removed_meshes.is_empty()
            && self.changed_materials.is_empty()
            && self.removed_materials.is_empty()
            && self.changed_instances.is_empty()
            && self.removed_instances.is_empty()
            && !self.changed_sky_environment
            && !self.changed_analytic_lights
    }
}

/// `SceneStore` 内部的合并型 change log。
///
/// 这里使用 set / map 保存本帧累计变化，避免同一个 handle 被多次 edit 时输出重复命令。
/// create 后同帧 delete 的 instance 会在 drain 前合并为 no-op，避免 render side 看见从未
/// 进入 prepare 边界的临时对象。
#[derive(Default)]
struct SceneChangeLog {
    removed_textures: HashSet<TextureHandle>,
    removed_meshes: HashSet<MeshHandle>,
    changed_materials: HashSet<MaterialHandle>,
    removed_materials: HashSet<MaterialHandle>,
    changed_instances: HashMap<InstanceHandle, SceneInstanceChangeKind>,
    created_instances: HashSet<InstanceHandle>,
    removed_instances: HashSet<InstanceHandle>,
    changed_sky_environment: bool,
    changed_analytic_lights: bool,
}

impl SceneChangeLog {
    fn mark_texture_removed(&mut self, handle: TextureHandle) {
        self.removed_textures.insert(handle);
    }

    fn mark_mesh_removed(&mut self, handle: MeshHandle) {
        self.removed_meshes.insert(handle);
    }

    fn mark_material_changed(&mut self, handle: MaterialHandle) {
        if !self.removed_materials.contains(&handle) {
            self.changed_materials.insert(handle);
        }
    }

    fn mark_material_removed(&mut self, handle: MaterialHandle) {
        self.changed_materials.remove(&handle);
        self.removed_materials.insert(handle);
    }

    fn mark_instance_changed(&mut self, handle: InstanceHandle, kind: SceneInstanceChangeKind) {
        if self.removed_instances.contains(&handle) {
            return;
        }
        self.changed_instances.entry(handle).and_modify(|current| *current = current.merge(kind)).or_insert(kind);
    }

    fn mark_instance_created(&mut self, handle: InstanceHandle) {
        self.created_instances.insert(handle);
        self.mark_instance_changed(handle, SceneInstanceChangeKind::Lifecycle);
    }

    fn mark_instance_removed(&mut self, handle: InstanceHandle) {
        if self.created_instances.remove(&handle) {
            // 该 instance 从未跨过 render sync 边界，render side 不应看见 create/delete 噪声。
            self.changed_instances.remove(&handle);
            return;
        }
        self.changed_instances.remove(&handle);
        self.removed_instances.insert(handle);
    }

    fn mark_analytic_lights_changed(&mut self) {
        self.changed_analytic_lights = true;
    }

    fn mark_sky_environment_changed(&mut self) {
        self.changed_sky_environment = true;
    }

    fn drain(&mut self) -> SceneChanges {
        let changes = std::mem::take(self);
        SceneChanges {
            removed_textures: changes.removed_textures.into_iter().collect(),
            removed_meshes: changes.removed_meshes.into_iter().collect(),
            changed_materials: changes.changed_materials.into_iter().collect(),
            removed_materials: changes.removed_materials.into_iter().collect(),
            changed_instances: changes
                .changed_instances
                .into_iter()
                .map(|(handle, kind)| SceneInstanceChange { handle, kind })
                .collect(),
            removed_instances: changes.removed_instances.into_iter().collect(),
            changed_sky_environment: changes.changed_sky_environment,
            changed_analytic_lights: changes.changed_analytic_lights,
        }
    }
}

/// CPU scene 内的 texture 语义记录。
///
/// v1 只需要 runtime 身份与生命周期；decoded CPU bytes 通过 `WorldRenderSync`
/// 短期流向 render-side texture manager，不进入这里的长期状态。
struct SceneTextureRecord;

/// CPU scene 内的 mesh 语义记录。
///
/// v1 只需要 runtime 身份与生命周期；vertex/index CPU data 通过 `WorldRenderSync`
/// 短期流向 render-side mesh manager，不进入这里的长期状态。
struct SceneMeshRecord;

/// CPU scene 内的 material 语义记录。
///
/// CPU 材质参数以 `MaterialData` 为权威值。GPU stable slot、dirty upload 和
/// texture fallback 仍属于 render-side material manager。
struct SceneMaterialRecord {
    data: MaterialData,
}

/// CPU scene 中的 sky / environment 权威状态。
///
/// 这里仅保存 App 可编辑的语义状态：是否启用、亮度参数、引用的 scene texture 以及语义版本。
/// GPU SRV、fallback texture、importance distribution 和 retired buffer 都属于 render-side
/// `RenderSkyManager`，不会进入 `SceneStore`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneSkyState {
    pub enabled: bool,
    pub intensity: f32,
    pub texture: Option<TextureHandle>,
    pub revision: u64,
}

/// 自发光 light table 构建所需的材质只读视图。
///
/// 该 view 直接借用 `SceneStore` 中的 CPU 权威材质参数，不复制完整材质列表，也不依赖
/// render-side material slot owner。GPU material slot 是否可见由 `RenderMaterialManager`
/// 的 resolver 单独提供。
#[derive(Clone, Copy)]
pub struct SceneMaterialEmissiveView<'a> {
    data: &'a MaterialData,
}

impl<'a> SceneMaterialEmissiveView<'a> {
    fn new(data: &'a MaterialData) -> Self {
        Self { data }
    }

    #[inline]
    pub fn base_color(&self) -> glam::Vec4 {
        self.data.base_color
    }

    #[inline]
    pub fn emissive(&self) -> glam::Vec4 {
        self.data.emissive
    }

    #[inline]
    pub fn opaque(&self) -> f32 {
        self.data.opaque
    }

    #[inline]
    pub fn diffuse_texture(&self) -> Option<TextureHandle> {
        self.data.diffuse_texture
    }
}

impl Default for SceneSkyState {
    fn default() -> Self {
        Self {
            enabled: true,
            intensity: 1.0,
            texture: None,
            revision: 0,
        }
    }
}

/// CPU scene 的只读视图。
///
/// 该 view 是 `World` 暴露给 render-side prepare 的窄接口：调用方只能读取当前
/// scene 快照，不能构造或修改 `SceneStore` owner。它不保存跨帧状态，也不拥有任何
/// loader / GPU resource。
#[derive(Clone, Copy)]
pub struct SceneReadView<'a> {
    scene: &'a SceneStore,
}

impl<'a> SceneReadView<'a> {
    pub(crate) fn new(scene: &'a SceneStore) -> Self {
        Self { scene }
    }

    /// 返回全部 live instance。
    ///
    /// 调用方不应把 map key 理解为 GPU slot；稳定 slot 由 render-side manager 独立维护。
    #[inline]
    pub fn instance_map(&self) -> &'a SlotMap<InstanceHandle, Instance> {
        &self.scene.all_instances
    }

    /// 返回全部 live point light。
    #[inline]
    pub fn point_light_map(&self) -> &'a SlotMap<LightHandle, gpu::light::PointLight> {
        &self.scene.all_point_lights
    }

    /// 返回全部 live spot light。
    #[inline]
    pub fn spot_light_map(&self) -> &'a SlotMap<LightHandle, gpu::light::SpotLight> {
        &self.scene.all_spot_lights
    }

    /// 返回全部 live area light。
    #[inline]
    pub fn area_light_map(&self) -> &'a SlotMap<LightHandle, gpu::light::AreaLight> {
        &self.scene.all_area_lights
    }

    /// 返回 analytic light 语义版本；只表达 point/spot/area 光源快照变化。
    #[inline]
    pub fn light_revision(&self) -> u32 {
        self.scene.light_revision
    }

    /// 返回 sky / environment 的 CPU 权威状态。
    #[inline]
    pub fn sky_state(&self) -> &'a SceneSkyState {
        &self.scene.sky_state
    }

    /// 按 scene material handle 返回自发光 table 需要的轻量材质 view。
    #[inline]
    pub fn material_emissive_view(&self, handle: MaterialHandle) -> Option<SceneMaterialEmissiveView<'a>> {
        self.scene.material_data(handle).map(SceneMaterialEmissiveView::new)
    }

    /// 按 scene material handle 查询 CPU 权威材质参数。
    #[inline]
    pub fn material_data(&self, handle: MaterialHandle) -> Option<&'a MaterialData> {
        self.scene.material_data(handle)
    }

    /// 按 CPU runtime handle 查询 live instance。
    #[inline]
    pub fn get_instance(&self, handle: InstanceHandle) -> Option<&'a Instance> {
        self.scene.all_instances.get(handle)
    }
}

/// CPU 侧 runtime scene 的所有者。
///
/// `SceneStore` 位于 `World` 的 scene 部分，负责保存 live instance / light 的语义状态。
/// 它只分配 `InstanceHandle` / `LightHandle` 这样的 runtime 身份，不创建 GPU 资源，也不解析
/// mesh、material 或 light 在 shader 中的可见绑定。渲染运行时的 `RenderInstanceManager` 会在
/// prepare/sync 阶段读取这里的数据，并维护 CPU handle 到 GPU scene slot 的映射。
#[derive(Default)]
pub(crate) struct SceneStore {
    /// scene texture 存储；key 是 CPU scene 长期引用，不表示 CPU bytes 或 GPU image ready。
    all_textures: SlotMap<TextureHandle, SceneTextureRecord>,
    /// scene mesh 存储；key 是 CPU scene 长期引用，不表示 GPU mesh ready。
    all_meshes: SlotMap<MeshHandle, SceneMeshRecord>,
    /// scene material 存储；key 是 CPU scene 长期引用，value 是 CPU 语义参数。
    all_materials: SlotMap<MaterialHandle, SceneMaterialRecord>,
    /// live instance 存储；slotmap key 是 CPU scene 内部的 runtime 身份。
    all_instances: SlotMap<InstanceHandle, Instance>,
    /// CPU sky / environment 权威状态。
    sky_state: SceneSkyState,
    /// texture -> material 反向依赖索引，只表达 CPU scene 语义引用。
    texture_to_materials: HashMap<TextureHandle, HashSet<MaterialHandle>>,
    /// material -> instance 反向依赖索引，用于删除拒绝与 render-side dirty routing。
    material_to_instances: HashMap<MaterialHandle, HashSet<InstanceHandle>>,
    /// mesh -> instance 反向依赖索引；v1 instance 创建后不支持修改 mesh 引用。
    mesh_to_instances: HashMap<MeshHandle, HashSet<InstanceHandle>>,
    /// live point light 存储；GPU 侧打包和上传由 render runtime 处理。
    all_point_lights: SlotMap<LightHandle, gpu::light::PointLight>,
    /// live spot light 存储；与 point light 分开保存，避免 CPU 语义层提前引入统一 light class。
    all_spot_lights: SlotMap<LightHandle, gpu::light::SpotLight>,
    /// live area light 存储；矩形单面发光的采样语义由 realtime RT shader 解释。
    all_area_lights: SlotMap<LightHandle, gpu::light::AreaLight>,
    /// point/spot/area light 语义变化版本，用于渲染端拒绝不匹配的 ReSTIR history。
    light_revision: u32,
    /// 本帧 CPU 语义变化；只在 `World::sync_for_render()` 中 drain。
    change_log: SceneChangeLog,
}
// 创建与初始化
impl SceneStore {
    /// 创建空的 CPU scene store。
    pub fn new() -> Self {
        Self::default()
    }
}
// 访问器
impl SceneStore {
    fn bump_light_revision(&mut self) {
        // 0 表示“尚未有 analytic light 语义版本”。第一次变化从 1 开始，便于 shader
        // reservoir metadata 把默认空状态和真实 scene 版本区分开；饱和后保持最大值即可触发不匹配。
        self.light_revision = self.light_revision.saturating_add(1).max(1);
    }

    fn bump_sky_revision(&mut self) {
        // sky revision 只表达 CPU 环境光语义变化；GPU distribution 版本仍由 RenderSkyManager
        // 独立维护，并在 scene root 中单独写入。
        self.sky_state.revision = self.sky_state.revision.saturating_add(1).max(1);
    }

    /// 判断 scene texture handle 是否仍属于当前 scene。
    #[inline]
    pub fn contains_texture(&self, handle: TextureHandle) -> bool {
        self.all_textures.contains_key(handle)
    }

    /// 判断 scene mesh handle 是否仍属于当前 scene。
    #[inline]
    pub fn contains_mesh(&self, handle: MeshHandle) -> bool {
        self.all_meshes.contains_key(handle)
    }

    /// 按 scene material handle 查询 CPU 权威材质参数。
    #[inline]
    pub fn material_data(&self, handle: MaterialHandle) -> Option<&MaterialData> {
        self.all_materials.get(handle).map(|record| &record.data)
    }

    /// 注册一个 scene texture 语义记录。
    pub fn register_texture(&mut self) -> TextureHandle {
        self.all_textures.insert(SceneTextureRecord)
    }

    /// 注册一个 scene mesh 语义记录。
    pub fn register_mesh(&mut self) -> MeshHandle {
        self.all_meshes.insert(SceneMeshRecord)
    }

    /// 删除一个 scene texture 语义记录。
    ///
    /// texture 仍被 material 或 sky 引用时必须拒绝删除。依赖检查通过前不修改 SlotMap、
    /// sky state 或 change log，确保删除失败时仍满足 scene edit 事务语义。
    pub fn remove_texture(&mut self, handle: TextureHandle) -> Result<(), SceneEditError> {
        if !self.all_textures.contains_key(handle) {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Texture,
            });
        }
        let material_dependents = self.texture_to_materials.get(&handle).map_or(0, HashSet::len);
        let sky_dependents = usize::from(self.sky_state.texture == Some(handle));
        let dependent_count = material_dependents + sky_dependents;
        if dependent_count > 0 {
            return Err(SceneEditError::StillReferenced {
                kind: SceneHandleKind::Texture,
                dependent_count,
            });
        }

        self.all_textures.remove(handle);
        self.texture_to_materials.remove(&handle);
        self.change_log.mark_texture_removed(handle);
        Ok(())
    }

    /// 更新 CPU sky 引用的 scene texture。
    pub fn update_sky_texture(&mut self, texture: Option<TextureHandle>) -> Result<(), SceneEditError> {
        if let Some(texture) = texture {
            if !self.all_textures.contains_key(texture) {
                return Err(SceneEditError::MissingDependency {
                    kind: SceneHandleKind::Texture,
                });
            }
        }
        if self.sky_state.texture == texture {
            return Ok(());
        }

        self.sky_state.texture = texture;
        self.bump_sky_revision();
        self.change_log.mark_sky_environment_changed();
        Ok(())
    }

    /// 更新 sky 是否参与 scene 环境光。
    pub fn update_sky_enabled(&mut self, enabled: bool) {
        if self.sky_state.enabled == enabled {
            return;
        }
        self.sky_state.enabled = enabled;
        self.bump_sky_revision();
        self.change_log.mark_sky_environment_changed();
    }

    /// 更新 sky 亮度语义参数。
    pub fn update_sky_intensity(&mut self, intensity: f32) {
        if self.sky_state.intensity == intensity {
            return;
        }
        self.sky_state.intensity = intensity;
        self.bump_sky_revision();
        self.change_log.mark_sky_environment_changed();
    }

    /// 删除一个 scene mesh 语义记录。
    pub fn remove_mesh(&mut self, handle: MeshHandle) -> Result<(), SceneEditError> {
        if !self.all_meshes.contains_key(handle) {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Mesh,
            });
        }
        let dependent_count = self.mesh_to_instances.get(&handle).map_or(0, HashSet::len);
        if dependent_count > 0 {
            return Err(SceneEditError::StillReferenced {
                kind: SceneHandleKind::Mesh,
                dependent_count,
            });
        }

        self.all_meshes.remove(handle);
        self.mesh_to_instances.remove(&handle);
        self.change_log.mark_mesh_removed(handle);
        Ok(())
    }

    /// 注册一个 scene material 语义记录。
    pub fn register_material(&mut self, data: MaterialData) -> Result<MaterialHandle, SceneEditError> {
        self.validate_material_texture_dependencies(&data)?;
        let handle = self.all_materials.insert(SceneMaterialRecord { data });
        let data = self.all_materials[handle].data.clone();
        self.add_material_texture_dependencies(handle, &data);
        self.change_log.mark_material_changed(handle);
        Ok(handle)
    }

    /// 更新一个 scene material 的 CPU 权威参数。
    pub fn update_material(&mut self, handle: MaterialHandle, data: MaterialData) -> Result<bool, SceneEditError> {
        self.validate_material_texture_dependencies(&data)?;
        let Some(old_data) = self.all_materials.get(handle).map(|record| record.data.clone()) else {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Material,
            });
        };
        if old_data == data {
            return Ok(false);
        }

        self.remove_material_texture_dependencies(handle, &old_data);
        self.add_material_texture_dependencies(handle, &data);
        let record = self.all_materials.get_mut(handle).expect("SceneStore: material disappeared after validation");
        record.data = data;
        self.change_log.mark_material_changed(handle);
        Ok(true)
    }

    /// 删除一个 scene material。
    pub fn remove_material(&mut self, handle: MaterialHandle) -> Result<(), SceneEditError> {
        let Some(record) = self.all_materials.get(handle) else {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Material,
            });
        };
        let dependent_count = self.material_to_instances.get(&handle).map_or(0, HashSet::len);
        if dependent_count > 0 {
            return Err(SceneEditError::StillReferenced {
                kind: SceneHandleKind::Material,
                dependent_count,
            });
        }

        let data = record.data.clone();
        self.all_materials.remove(handle);
        self.remove_material_texture_dependencies(handle, &data);
        self.material_to_instances.remove(&handle);
        self.change_log.mark_material_removed(handle);
        Ok(())
    }

    /// 向 CPU scene 添加一个 live instance，并返回它的 runtime 身份。
    ///
    /// 注册只改变 CPU 语义状态；mesh/material asset 是否已经 GPU-ready 由 render-side
    /// bridge 在同步时检查。
    pub fn register_instance(&mut self, instance: Instance) -> Result<InstanceHandle, SceneEditError> {
        self.validate_instance_dependencies(&instance)?;
        let handle = self.all_instances.insert(instance);
        let instance = self.all_instances.get(handle).expect("SceneStore: instance disappeared after insert").clone();
        self.add_instance_dependencies(handle, &instance);
        self.change_log.mark_instance_created(handle);
        Ok(handle)
    }

    /// 从 CPU scene 移除 live instance。
    ///
    /// 返回的 instance 数据只代表 CPU 记录。已建立的 GPU-side 映射会在后续 prepare/sync
    /// 阶段被 `RenderInstanceManager` 识别为 stale 并回收。
    pub fn remove_instance(&mut self, handle: InstanceHandle) -> Result<(), SceneEditError> {
        let Some(instance) = self.all_instances.remove(handle) else {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Instance,
            });
        };
        self.remove_instance_dependencies(handle, &instance);
        self.change_log.mark_instance_removed(handle);
        Ok(())
    }

    /// 更新 live instance 的 CPU world transform。
    ///
    /// 返回 `false` 表示 handle 已失效或不属于当前 scene。GPU scene 数据不会在这里直接写入，
    /// 而是在下一次 render runtime 同步时更新。
    pub fn update_instance_transform(
        &mut self,
        handle: InstanceHandle,
        transform: glam::Mat4,
    ) -> Result<(), SceneEditError> {
        let Some(instance) = self.all_instances.get_mut(handle) else {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Instance,
            });
        };
        if instance.transform == transform {
            return Ok(());
        }
        instance.transform = transform;
        self.change_log.mark_instance_changed(handle, SceneInstanceChangeKind::Transform);
        Ok(())
    }

    /// 更新 instance 的 material 列表。
    ///
    /// v1 不允许更新 instance mesh 引用，因此这里只维护 material -> instance 反向依赖和
    /// `MaterialBinding` change。mesh 反向依赖保持创建时的关系不变。
    pub fn update_instance_materials(
        &mut self,
        handle: InstanceHandle,
        materials: Vec<MaterialHandle>,
    ) -> Result<(), SceneEditError> {
        self.validate_material_handles(&materials)?;
        let Some(old_instance) = self.all_instances.get(handle).cloned() else {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Instance,
            });
        };
        if old_instance.materials == materials {
            return Ok(());
        }

        self.remove_instance_material_dependencies(handle, &old_instance.materials);
        self.add_instance_material_dependencies(handle, &materials);
        let instance =
            self.all_instances.get_mut(handle).expect("SceneStore: instance disappeared after dependency validation");
        instance.materials = materials;
        self.change_log.mark_instance_changed(handle, SceneInstanceChangeKind::MaterialBinding);
        Ok(())
    }

    /// 向 CPU scene 添加一个 live point light。
    ///
    /// 光源使用 shader binding 中的共享布局类型，但这里仍只负责 CPU 侧生命周期；GPU buffer
    /// 更新由 render runtime 的 scene 同步流程处理。
    pub fn register_point_light(&mut self, light: gpu::light::PointLight) -> LightHandle {
        let handle = self.all_point_lights.insert(light);
        self.bump_light_revision();
        self.change_log.mark_analytic_lights_changed();
        handle
    }

    /// 向 CPU scene 添加一个 live spot light。
    ///
    /// spot light 在 realtime RT 中表示半径固定为 0.5 的 sphere emitter，并额外带 cone
    /// falloff；这里不做角度或方向归一化，调用方和 shader ABI 注释共同约束输入单位。
    pub fn register_spot_light(&mut self, light: gpu::light::SpotLight) -> LightHandle {
        let handle = self.all_spot_lights.insert(light);
        self.bump_light_revision();
        self.change_log.mark_analytic_lights_changed();
        handle
    }

    /// 向 CPU scene 添加一个 live area light。
    ///
    /// area light 使用 world-space `center + half_u + half_v` 描述矩形；本 manager 不计算
    /// 法线或面积，避免 CPU scene 与 shader 采样路径维护两套几何派生规则。
    pub fn register_area_light(&mut self, light: gpu::light::AreaLight) -> LightHandle {
        let handle = self.all_area_lights.insert(light);
        self.bump_light_revision();
        self.change_log.mark_analytic_lights_changed();
        handle
    }

    /// 返回并清空本帧 CPU 语义变化。
    ///
    /// 只有 `World::sync_for_render()` 应调用该方法；render side 不直接 drain `SceneStore`，
    /// 避免 CPU scene change 和 asset upload payload 被两个阶段分别消费。
    pub fn drain_changes(&mut self) -> SceneChanges {
        self.change_log.drain()
    }
}

// 依赖索引与 edit 事务校验
impl SceneStore {
    fn validate_material_texture_dependencies(&self, data: &MaterialData) -> Result<(), SceneEditError> {
        for texture in Self::material_texture_handles(data) {
            if !self.all_textures.contains_key(texture) {
                return Err(SceneEditError::MissingDependency {
                    kind: SceneHandleKind::Texture,
                });
            }
        }
        Ok(())
    }

    fn validate_material_handles(&self, materials: &[MaterialHandle]) -> Result<(), SceneEditError> {
        for &material in materials {
            if !self.all_materials.contains_key(material) {
                return Err(SceneEditError::MissingDependency {
                    kind: SceneHandleKind::Material,
                });
            }
        }
        Ok(())
    }

    fn validate_instance_dependencies(&self, instance: &Instance) -> Result<(), SceneEditError> {
        if !self.all_meshes.contains_key(instance.mesh) {
            return Err(SceneEditError::MissingDependency {
                kind: SceneHandleKind::Mesh,
            });
        }
        self.validate_material_handles(&instance.materials)
    }

    fn material_texture_handles(data: &MaterialData) -> impl Iterator<Item = TextureHandle> {
        [data.diffuse_texture, data.normal_texture].into_iter().flatten()
    }

    fn add_material_texture_dependencies(&mut self, material: MaterialHandle, data: &MaterialData) {
        for texture in Self::material_texture_handles(data) {
            self.texture_to_materials.entry(texture).or_default().insert(material);
        }
    }

    fn remove_material_texture_dependencies(&mut self, material: MaterialHandle, data: &MaterialData) {
        for texture in Self::material_texture_handles(data) {
            Self::remove_reverse_dependency(&mut self.texture_to_materials, texture, material);
        }
    }

    fn add_instance_dependencies(&mut self, instance_handle: InstanceHandle, instance: &Instance) {
        self.mesh_to_instances.entry(instance.mesh).or_default().insert(instance_handle);
        self.add_instance_material_dependencies(instance_handle, &instance.materials);
    }

    fn remove_instance_dependencies(&mut self, instance_handle: InstanceHandle, instance: &Instance) {
        Self::remove_reverse_dependency(&mut self.mesh_to_instances, instance.mesh, instance_handle);
        self.remove_instance_material_dependencies(instance_handle, &instance.materials);
    }

    fn add_instance_material_dependencies(&mut self, instance_handle: InstanceHandle, materials: &[MaterialHandle]) {
        for &material in materials {
            self.material_to_instances.entry(material).or_default().insert(instance_handle);
        }
    }

    fn remove_instance_material_dependencies(&mut self, instance_handle: InstanceHandle, materials: &[MaterialHandle]) {
        for &material in materials {
            Self::remove_reverse_dependency(&mut self.material_to_instances, material, instance_handle);
        }
    }

    fn remove_reverse_dependency<K, V>(map: &mut HashMap<K, HashSet<V>>, key: K, value: V)
    where
        K: Eq + std::hash::Hash + Copy,
        V: Eq + std::hash::Hash + Copy,
    {
        let Some(dependents) = map.get_mut(&key) else {
            return;
        };
        dependents.remove(&value);
        if dependents.is_empty() {
            map.remove(&key);
        }
    }
}
impl Drop for SceneStore {
    fn drop(&mut self) {
        log::info!("SceneStore dropped.");
    }
}
// 销毁
impl SceneStore {
    /// 清空 CPU scene 记录，供拥有者按既有 destroy 顺序显式释放。
    pub fn destroy_mut(&mut self) {
        // destroy 只在确实存在 analytic light 时推进版本，避免空 scene 关闭路径无意义改变
        // GPU scene signature。已有 history 在后续 frame 也会因为 light count/key 判界失败而失效。
        let had_lights =
            !self.all_point_lights.is_empty() || !self.all_spot_lights.is_empty() || !self.all_area_lights.is_empty();
        self.all_textures.clear();
        self.all_meshes.clear();
        self.all_materials.clear();
        self.all_instances.clear();
        self.sky_state = SceneSkyState::default();
        self.texture_to_materials.clear();
        self.material_to_instances.clear();
        self.mesh_to_instances.clear();
        self.all_point_lights.clear();
        self.all_spot_lights.clear();
        self.all_area_lights.clear();
        self.change_log = SceneChangeLog::default();
        if had_lights {
            self.bump_light_revision();
            self.change_log.mark_analytic_lights_changed();
        }
    }
}
