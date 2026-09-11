use std::collections::{HashMap, HashSet};

use slotmap::{SecondaryMap, SlotMap};

use truvis_asset::handle::{MeshData, TextureBytes};
use truvis_shader_binding::gpu;

use crate::components::instance::Instance;
use crate::components::material::MaterialData;
use crate::edit_error::{SceneEditError, SceneHandleKind};
use crate::guid_new_type::{InstanceHandle, LightHandle, MaterialHandle, MeshHandle, TextureHandle};
use crate::asset_system::AssetStore;

/// CPU scene 中的 sky / environment 权威状态。
///
/// 这里仅保存 Renderer 可编辑的语义状态：是否启用、引用的 scene texture 以及语义版本。
/// GPU SRV、fallback texture、importance distribution 和 retired buffer 都属于 render-side
/// `GpuSkyStore`，不会进入 `SceneStore`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneSkyState {
    pub enabled: bool,
    pub texture: Option<TextureHandle>,
    pub revision: u64,
}

impl Default for SceneSkyState {
    fn default() -> Self {
        Self {
            enabled: true,
            texture: None,
            revision: 0,
        }
    }
}

/// CPU scene 的只读视图。
///
/// 该 view 是 `GameWorld` 暴露给 render-side prepare 的窄接口：调用方只能读取当前
/// scene 快照，不能构造或修改 `SceneStore` owner。它不保存跨帧状态，也不拥有任何
/// loader / GPU resource。
#[derive(Clone, Copy)]
pub struct SceneReadView<'a> {
    scene: &'a SceneStore,
    resources: &'a AssetStore,
}

impl<'a> SceneReadView<'a> {
    pub(crate) fn new(scene: &'a SceneStore, resources: &'a AssetStore) -> Self {
        Self { scene, resources }
    }

    /// 返回全部 live instance。
    ///
    /// 调用方不应把 map key 理解为 GPU slot；稳定 slot 由 render-side manager 独立维护。
    #[inline]
    pub fn instance_map(&self) -> &'a SlotMap<InstanceHandle, Instance> {
        &self.scene.all_instances
    }

    /// 返回全部 live material handle，供 render-side 做完整状态对账。
    #[inline]
    pub fn material_handles(&self) -> impl Iterator<Item = MaterialHandle> + 'a {
        self.resources.material_handles()
    }

    /// 返回全部 live texture handle，供 render-side 清理已删除的 GPU 记录。
    #[inline]
    pub fn texture_handles(&self) -> impl Iterator<Item = TextureHandle> + 'a {
        self.resources.texture_handles()
    }

    #[inline]
    pub fn contains_texture(&self, handle: TextureHandle) -> bool {
        self.resources.contains_texture(handle)
    }

    /// 返回已经完成 CPU 解码的纹理内容；未完成或失败的纹理返回 `None`。
    #[inline]
    pub fn texture_data(&self, handle: TextureHandle) -> Option<&'a TextureBytes> {
        self.resources.texture_data(handle)
    }

    /// 返回全部 live mesh handle，供 render-side 清理已删除的 GPU 记录。
    #[inline]
    pub fn mesh_handles(&self) -> impl Iterator<Item = MeshHandle> + 'a {
        self.resources.mesh_handles()
    }

    #[inline]
    pub fn contains_mesh(&self, handle: MeshHandle) -> bool {
        self.resources.contains_mesh(handle)
    }

    /// 返回不可变 CPU mesh 内容，供 render resource owner 首次安装或重试上传。
    #[inline]
    pub fn mesh_data(&self, handle: MeshHandle) -> Option<&'a MeshData> {
        self.resources.mesh_data(handle)
    }

    /// 返回 CPU scene 的单调递增全局语义版本。
    ///
    /// 该版本不会在 prepare 时清零，也不表示 GPU prepare 已完成；Editor
    /// 只用它检测多次最终一致查询之间是否发生 CPU 语义变化。
    #[inline]
    pub fn scene_version(&self) -> u64 {
        self.scene.scene_version
    }

    /// 返回全部 live point light。
    #[inline]
    pub fn point_light_map(&self) -> &'a SlotMap<LightHandle, gpu::engine::light::PointLight> {
        &self.scene.all_point_lights
    }

    /// 返回全部 live spot light。
    #[inline]
    pub fn spot_light_map(&self) -> &'a SlotMap<LightHandle, gpu::engine::light::SpotLight> {
        &self.scene.all_spot_lights
    }

    /// 返回全部 live area light。
    #[inline]
    pub fn area_light_map(&self) -> &'a SlotMap<LightHandle, gpu::engine::light::AreaLight> {
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

    /// 按 scene material handle 查询 CPU 权威材质参数。
    #[inline]
    pub fn material_data(&self, handle: MaterialHandle) -> Option<&'a MaterialData> {
        self.resources.material_data(handle)
    }

    /// 返回材质内容 revision；它不表示 GPU slot 或 shader-visible buffer 已经更新。
    #[inline]
    pub fn material_revision(&self, handle: MaterialHandle) -> Option<u64> {
        self.resources.material_revision(handle)
    }

    /// 按 CPU runtime handle 查询 live instance。
    #[inline]
    pub fn get_instance(&self, handle: InstanceHandle) -> Option<&'a Instance> {
        self.scene.all_instances.get(handle)
    }

    /// 返回 instance 最近一次有效编辑的 source revision。
    #[inline]
    pub fn instance_revision(&self, handle: InstanceHandle) -> Option<u64> {
        self.scene.instance_revisions.get(handle).copied()
    }

    /// 按 CPU scene mesh handle 查询长期保存的展示名称。
    ///
    /// 这里只暴露编辑器需要的语义 metadata，不暴露 `SceneMeshRecord` owner，也不表示
    /// render-side mesh 已经完成 GPU upload 或 BLAS build。
    #[inline]
    pub fn mesh_name(&self, handle: MeshHandle) -> Option<&'a str> {
        self.resources.mesh_name(handle)
    }

    /// 查询直接引用指定 texture 的 material。
    ///
    /// 这是 render-side 资源对账使用的只读依赖视图。调用方只能拿到当前
    /// CPU scene 语义下的 handle 列表，不能接触内部反向索引 owner。
    pub fn materials_using_texture(&self, texture: TextureHandle) -> impl Iterator<Item = MaterialHandle> + 'a {
        self.resources.materials_using_texture(texture)
    }

    /// 查询直接引用指定 material 的 instance。
    ///
    /// 结果只表达 CPU scene 语义依赖；instance 是否已经 GPU-ready 仍由 render-side
    /// `RenderInstanceTable` 和 resolver 在 prepare 阶段判断。
    pub fn instances_using_material(&self, material: MaterialHandle) -> impl Iterator<Item = InstanceHandle> + 'a {
        self.scene.material_to_instances.get(&material).into_iter().flat_map(|instances| instances.iter().copied())
    }

    /// 查询直接引用指定 mesh 的 instance。
    pub fn instances_using_mesh(&self, mesh: MeshHandle) -> impl Iterator<Item = InstanceHandle> + 'a {
        self.scene.mesh_to_instances.get(&mesh).into_iter().flat_map(|instances| instances.iter().copied())
    }

    /// 判断当前 sky / environment 是否引用指定 texture。
    #[inline]
    pub fn sky_uses_texture(&self, texture: TextureHandle) -> bool {
        self.scene.sky_state.texture == Some(texture)
    }
}

/// CPU 侧 runtime scene 的所有者。
///
/// `SceneStore` 只保存 live instance / light / sky 以及场景关系索引。mesh、material、texture
/// 的身份与内容由同一个 `GameWorld` 中的 `AssetSystem` 持有；这里仅通过 handle 建立组合关系。
/// 它不创建 GPU 资源，也不解析资源在 shader 中的可见绑定。
#[derive(Default)]
pub(crate) struct SceneStore {
    /// CPU scene 的全局语义版本；只在实际 mutation 成功后推进，失败和 no-op 不推进。
    scene_version: u64,
    /// live instance 存储；slotmap key 是 CPU scene 内部的 runtime 身份。
    all_instances: SlotMap<InstanceHandle, Instance>,
    /// 每个 instance 的 source revision；只在有效 transform/material 编辑后推进。
    instance_revisions: SecondaryMap<InstanceHandle, u64>,
    /// CPU sky / environment 权威状态。
    sky_state: SceneSkyState,
    /// material -> instance 反向依赖索引，用于查询与删除拒绝。
    material_to_instances: HashMap<MaterialHandle, HashSet<InstanceHandle>>,
    /// mesh -> instance 反向依赖索引；v1 instance 创建后不支持修改 mesh 引用。
    mesh_to_instances: HashMap<MeshHandle, HashSet<InstanceHandle>>,
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
impl SceneStore {
    fn bump_scene_version(&mut self) {
        // u64 饱和在实际工程生命周期内不可达；使用饱和加法保持“不会回退”的协议契约。
        self.scene_version = self.scene_version.saturating_add(1);
    }

    pub(crate) fn mark_resource_changed(&mut self) {
        self.bump_scene_version();
    }

    pub(crate) fn instance_dependents_for_material(&self, material: MaterialHandle) -> usize {
        self.material_to_instances.get(&material).map_or(0, HashSet::len)
    }

    pub(crate) fn instance_dependents_for_mesh(&self, mesh: MeshHandle) -> usize {
        self.mesh_to_instances.get(&mesh).map_or(0, HashSet::len)
    }

    pub(crate) fn sky_uses_texture(&self, texture: TextureHandle) -> bool {
        self.sky_state.texture == Some(texture)
    }

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
        // sky revision 只表达 CPU 环境光语义变化；GPU distribution 版本仍由 GpuSkyStore
        // 独立维护，并在 scene root 中单独写入。
        self.sky_state.revision = self.sky_state.revision.saturating_add(1).max(1);
    }

    /// 更新 CPU sky 引用的 scene texture。
    pub fn update_sky_texture(&mut self, texture: Option<TextureHandle>, resources: &AssetStore) -> Result<(), SceneEditError> {
        if let Some(texture) = texture {
            if !resources.contains_texture(texture) {
                return Err(SceneEditError::MissingDependency { kind: SceneHandleKind::Texture });
            }
        }
        if self.sky_state.texture == texture {
            return Ok(());
        }

        self.sky_state.texture = texture;
        self.bump_sky_revision();
        self.bump_scene_version();
        Ok(())
    }

    /// 更新 sky 是否参与 scene 环境光。
    pub fn update_sky_enabled(&mut self, enabled: bool) {
        if self.sky_state.enabled == enabled {
            return;
        }
        self.sky_state.enabled = enabled;
        self.bump_sky_revision();
        self.bump_scene_version();
    }

    /// 向 CPU scene 添加一个 live instance，并返回它的 runtime 身份。
    ///
    /// 注册只改变 CPU 语义状态；mesh/material asset 是否已经 GPU-ready 由 render-side
    /// bridge 在同步时检查。
    pub fn register_instance(&mut self, resources: &AssetStore, instance: Instance) -> Result<InstanceHandle, SceneEditError> {
        self.validate_instance_dependencies(resources, &instance)?;
        let handle = self.all_instances.insert(instance);
        self.instance_revisions.insert(handle, 1);
        let instance = self.all_instances.get(handle).expect("SceneStore: instance disappeared after insert").clone();
        self.add_instance_dependencies(handle, &instance);
        self.bump_scene_version();
        Ok(handle)
    }

    /// 从 CPU scene 移除 live instance。
    ///
    /// 返回的 instance 数据只代表 CPU 记录。已建立的 GPU-side 映射会在后续 prepare/sync
    /// 阶段被 `RenderInstanceTable` 识别为 stale 并回收。
    pub fn remove_instance(&mut self, handle: InstanceHandle) -> Result<(), SceneEditError> {
        let Some(instance) = self.all_instances.remove(handle) else {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Instance,
            });
        };
        self.instance_revisions.remove(handle);
        self.remove_instance_dependencies(handle, &instance);
        self.bump_scene_version();
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
        let revision = self.instance_revisions.get_mut(handle).expect("SceneStore: instance revision missing");
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
        resources: &AssetStore,
        handle: InstanceHandle,
        materials: Vec<MaterialHandle>,
    ) -> Result<(), SceneEditError> {
        let Some(old_instance) = self.all_instances.get(handle).cloned() else {
            return Err(SceneEditError::StaleHandle {
                kind: SceneHandleKind::Instance,
            });
        };
        self.validate_material_handles(resources, &materials)?;
        self.validate_instance_material_count(resources, old_instance.mesh, materials.len())?;
        if old_instance.materials == materials {
            return Ok(());
        }

        self.remove_instance_material_dependencies(handle, &old_instance.materials);
        self.add_instance_material_dependencies(handle, &materials);
        let instance =
            self.all_instances.get_mut(handle).expect("SceneStore: instance disappeared after dependency validation");
        instance.materials = materials;
        let revision = self.instance_revisions.get_mut(handle).expect("SceneStore: instance revision missing");
        *revision = revision.saturating_add(1).max(1);
        self.bump_scene_version();
        Ok(())
    }

    /// 向 CPU scene 添加一个 live point light。
    ///
    /// 光源使用 shader binding 中的共享布局类型，但这里仍只负责 CPU 侧生命周期；GPU buffer
    /// 更新由 render runtime 的 scene 同步流程处理。
    pub fn register_point_light(&mut self, light: gpu::engine::light::PointLight) -> LightHandle {
        let handle = self.all_point_lights.insert(light);
        self.bump_light_revision();
        self.bump_scene_version();
        handle
    }

    /// 向 CPU scene 添加一个 live spot light。
    ///
    /// spot light 在 realtime RT 中表示半径固定为 0.5 的 sphere emitter，并额外带 cone
    /// falloff；这里不做角度或方向归一化，调用方和 shader ABI 注释共同约束输入单位。
    pub fn register_spot_light(&mut self, light: gpu::engine::light::SpotLight) -> LightHandle {
        let handle = self.all_spot_lights.insert(light);
        self.bump_light_revision();
        self.bump_scene_version();
        handle
    }

    /// 向 CPU scene 添加一个 live area light。
    ///
    /// area light 使用 world-space `center + half_u + half_v` 描述矩形；本 manager 不计算
    /// 法线或面积，避免 CPU scene 与 shader 采样路径维护两套几何派生规则。
    pub fn register_area_light(&mut self, light: gpu::engine::light::AreaLight) -> LightHandle {
        let handle = self.all_area_lights.insert(light);
        self.bump_light_revision();
        self.bump_scene_version();
        handle
    }

}

// 场景关系索引与 edit 校验
impl SceneStore {
    fn validate_material_handles(&self, resources: &AssetStore, materials: &[MaterialHandle]) -> Result<(), SceneEditError> {
        for &material in materials {
            if !resources.contains_material(material) {
                return Err(SceneEditError::MissingDependency {
                    kind: SceneHandleKind::Material,
                });
            }
        }
        Ok(())
    }

    fn validate_instance_dependencies(&self, resources: &AssetStore, instance: &Instance) -> Result<(), SceneEditError> {
        if !resources.contains_mesh(instance.mesh) {
            return Err(SceneEditError::MissingDependency {
                kind: SceneHandleKind::Mesh,
            });
        }
        self.validate_material_handles(resources, &instance.materials)?;
        self.validate_instance_material_count(resources, instance.mesh, instance.materials.len())
    }

    fn validate_instance_material_count(
        &self,
        resources: &AssetStore,
        mesh: MeshHandle,
        material_count: usize,
    ) -> Result<(), SceneEditError> {
        let Some(expected) = resources.mesh_submesh_count(mesh) else {
            return Err(SceneEditError::MissingDependency {
                kind: SceneHandleKind::Mesh,
            });
        };
        if expected != material_count {
            return Err(SceneEditError::MaterialCountMismatch {
                expected,
                actual: material_count,
            });
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use crate::components::material::{CoverageMode, MaterialClass};
    use truvis_asset::handle::{MeshData, SubmeshData};

    use super::*;

    fn test_submesh(name: &str) -> SubmeshData {
        SubmeshData {
            positions: vec![
                glam::vec3(0.0, 0.0, 0.0),
                glam::vec3(1.0, 0.0, 0.0),
                glam::vec3(0.0, 1.0, 0.0),
            ],
            normals: vec![glam::Vec3::Z; 3],
            tangents: vec![glam::Vec3::X; 3],
            uvs: vec![glam::Vec2::ZERO; 3],
            indices: vec![0, 1, 2],
            name: name.to_string(),
        }
    }

    fn test_mesh(submesh_count: usize) -> MeshData {
        MeshData {
            name: "test-mesh".to_string(),
            submeshes: (0..submesh_count).map(|index| test_submesh(&format!("submesh-{index}"))).collect(),
        }
    }

    fn test_material(name: &str) -> MaterialData {
        MaterialData {
            base_color: glam::Vec4::ONE,
            metallic: 0.0,
            roughness: 0.5,
            class: MaterialClass::Surface,
            coverage: CoverageMode::Opaque,
            diffuse_texture: None,
            normal_texture: None,
            name: name.to_string(),
        }
    }

    #[test]
    fn rejects_mesh_without_submeshes() {
        let mut resources = AssetStore::default();
        let err = resources.register_mesh(test_mesh(0)).unwrap_err();
        assert!(matches!(err, SceneEditError::InvalidMeshData { .. }));
    }

    #[test]
    fn validates_instance_material_count_against_mesh_submeshes() {
        let mut scene = SceneStore::new();
        let mut resources = AssetStore::default();
        let mesh = resources.register_mesh(test_mesh(2)).unwrap();
        let material_a = resources.register_material(test_material("a")).unwrap();
        let material_b = resources.register_material(test_material("b")).unwrap();

        let err = scene
            .register_instance(&resources, Instance {
                name: "invalid-material-count".to_string(),
                mesh,
                materials: vec![material_a],
                transform: glam::Mat4::IDENTITY,
            })
            .unwrap_err();
        assert_eq!(err, SceneEditError::MaterialCountMismatch { expected: 2, actual: 1 });

        let instance = scene
            .register_instance(&resources, Instance {
                name: "valid-material-count".to_string(),
                mesh,
                materials: vec![material_a, material_b],
                transform: glam::Mat4::IDENTITY,
            })
            .unwrap();

        let err = scene.update_instance_materials(&resources, instance, vec![material_a]).unwrap_err();
        assert_eq!(err, SceneEditError::MaterialCountMismatch { expected: 2, actual: 1 });
    }

    #[test]
    fn tracks_reverse_references_until_instance_is_removed() {
        let mut scene = SceneStore::new();
        let mut resources = AssetStore::default();
        let mesh = resources.register_mesh(test_mesh(1)).unwrap();
        let material = resources.register_material(test_material("material")).unwrap();

        let instance = scene
            .register_instance(&resources, Instance {
                name: "referenced".to_string(),
                mesh,
                materials: vec![material],
                transform: glam::Mat4::IDENTITY,
            })
            .unwrap();

        assert_eq!(scene.instance_dependents_for_mesh(mesh), 1);
        assert_eq!(scene.instance_dependents_for_material(material), 1);
        assert_eq!(scene.mesh_to_instances.get(&mesh).unwrap().iter().copied().collect::<Vec<_>>(), vec![instance]);
        assert_eq!(scene.material_to_instances.get(&material).unwrap().iter().copied().collect::<Vec<_>>(), vec![instance]);

        assert_eq!(scene.remove_instance(instance), Ok(()));
        assert_eq!(scene.instance_dependents_for_mesh(mesh), 0);
        assert_eq!(scene.instance_dependents_for_material(material), 0);
        assert!(scene.mesh_to_instances.get(&mesh).is_none());
        assert!(scene.material_to_instances.get(&material).is_none());

        resources.remove_material(material, 0).unwrap();
        resources.remove_mesh(mesh, 0).unwrap();
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
        self.all_instances.clear();
        self.instance_revisions.clear();
        self.sky_state = SceneSkyState::default();
        self.material_to_instances.clear();
        self.mesh_to_instances.clear();
        self.all_point_lights.clear();
        self.all_spot_lights.clear();
        self.all_area_lights.clear();
        if had_lights {
            self.bump_light_revision();
        }
    }
}
