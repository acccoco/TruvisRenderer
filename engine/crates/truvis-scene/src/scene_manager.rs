use slotmap::SlotMap;

use truvis_asset::handle::{AssetMaterialHandle, AssetMeshHandle, LoadedSceneData};
use truvis_gfx::gfx::{GfxDeviceCtx, GfxResourceCtx};
use truvis_render_interface::render_data::MeshRenderData;
use truvis_shader_binding::gpu;

use crate::components::instance::Instance;
use crate::components::material::Material;
use crate::components::mesh::Mesh;
use crate::guid_new_type::{InstanceHandle, LightHandle, MaterialHandle, MeshHandle};

/// asset material handle 到稳定 GPU material slot 的解析接口。
///
/// 由 render-side material bridge 实现，scene 层只依赖 slot 结果，
/// 不接触 texture、bindless 或 GPU material buffer 的细节。
pub trait MaterialSlotResolver {
    fn resolve_material_slot(&self, handle: AssetMaterialHandle) -> Option<u32>;

    fn is_material_ready(&self, handle: AssetMaterialHandle) -> bool {
        self.resolve_material_slot(handle).is_some()
    }
}

/// asset mesh handle 到 GPU-ready mesh 数据的解析接口。
///
/// 由 render-side mesh uploader 实现，scene 层只依赖资产 mesh 是否已经可渲染，
/// 不接触 vertex/index buffer 上传或 BLAS 构建细节。
pub trait MeshRenderResolver {
    fn is_mesh_ready(&self, handle: AssetMeshHandle) -> bool {
        self.resolve_mesh(handle).is_some()
    }

    fn resolve_mesh(&self, handle: AssetMeshHandle) -> Option<MeshRenderData<'_>>;
}

/// 在 CPU 侧管理场景数据
#[derive(Default)]
pub struct SceneManager {
    all_mats: SlotMap<MaterialHandle, Material>,
    all_instances: SlotMap<InstanceHandle, Instance>,
    all_meshes: SlotMap<MeshHandle, Mesh>,

    all_point_lights: SlotMap<LightHandle, gpu::PointLight>,
}
// 创建与初始化
impl SceneManager {
    pub fn new() -> Self {
        Self::default()
    }
}
// 访问器
impl SceneManager {
    #[inline]
    pub fn mat_map(&self) -> &SlotMap<MaterialHandle, Material> {
        &self.all_mats
    }
    #[inline]
    pub fn instance_map(&self) -> &SlotMap<InstanceHandle, Instance> {
        &self.all_instances
    }
    #[inline]
    pub fn mesh_map(&self) -> &SlotMap<MeshHandle, Mesh> {
        &self.all_meshes
    }
    #[inline]
    pub fn point_light_map(&self) -> &SlotMap<LightHandle, gpu::PointLight> {
        &self.all_point_lights
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.all_instances.is_empty()
            && self.all_meshes.is_empty()
            && self.all_mats.is_empty()
            && self.all_point_lights.is_empty()
    }
}
// 工具函数
impl SceneManager {
    #[inline]
    pub fn get_instance(&self, handle: InstanceHandle) -> Option<&Instance> {
        self.all_instances.get(handle)
    }

    #[inline]
    pub fn get_mesh(&self, handle: MeshHandle) -> Option<&Mesh> {
        self.all_meshes.get(handle)
    }

    #[inline]
    pub fn get_material(&self, handle: MaterialHandle) -> Option<&Material> {
        self.all_mats.get(handle)
    }

    /// 向场景中添加材质
    pub fn register_mat(&mut self, mat: Material) -> MaterialHandle {
        self.all_mats.insert(mat)
    }

    /// 向场景中添加 mesh
    pub fn register_mesh(&mut self, mesh: Mesh) -> MeshHandle {
        self.all_meshes.insert(mesh)
    }

    /// 向场景中添加 instance
    pub fn register_instance(&mut self, instance: Instance) -> InstanceHandle {
        self.all_instances.insert(instance)
    }

    /// 将 scene asset / prefab spawn 为 runtime instances。
    ///
    /// `LoadedSceneData` 不持有 live instance 生命周期；每次调用都会创建一组新的
    /// `InstanceHandle`，由 `SceneManager` 独立管理。
    pub fn spawn_scene_asset(&mut self, scene_data: &LoadedSceneData) -> Vec<InstanceHandle> {
        scene_data
            .instances
            .iter()
            .map(|instance| {
                self.register_instance(Instance {
                    mesh: instance.mesh,
                    materials: instance.materials.clone(),
                    transform: instance.transform,
                })
            })
            .collect()
    }

    pub fn remove_instance(&mut self, handle: InstanceHandle) -> Option<Instance> {
        self.all_instances.remove(handle)
    }

    pub fn update_instance_transform(&mut self, handle: InstanceHandle, transform: glam::Mat4) -> bool {
        let Some(instance) = self.all_instances.get_mut(handle) else {
            return false;
        };
        instance.transform = transform;
        true
    }

    /// 向场景中添加点光源
    pub fn register_point_light(&mut self, light: gpu::PointLight) -> LightHandle {
        self.all_point_lights.insert(light)
    }
}
impl Drop for SceneManager {
    fn drop(&mut self) {
        log::info!("SceneManager dropped.");
    }
}
// 销毁
impl SceneManager {
    pub fn destroy(mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        self.destroy_mut(resource_ctx, device_ctx);
    }
    pub fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        let _ = (resource_ctx, device_ctx);
        self.all_mats.clear();
        self.all_instances.clear();
        self.all_meshes.clear();
        self.all_point_lights.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use slotmap::SlotMap;
    use truvis_asset::handle::{AssetMaterialHandle, AssetMeshHandle, LoadedSceneData, LoadedSceneInstanceData};

    use super::*;

    fn mesh_handle() -> AssetMeshHandle {
        SlotMap::<AssetMeshHandle, ()>::with_key().insert(())
    }

    fn material_handle() -> AssetMaterialHandle {
        SlotMap::<AssetMaterialHandle, ()>::with_key().insert(())
    }

    fn scene_data(mesh: AssetMeshHandle, material: AssetMaterialHandle) -> LoadedSceneData {
        LoadedSceneData {
            source_path: PathBuf::from("assets/model.fbx"),
            name: "model.fbx".to_string(),
            meshes: vec![mesh],
            materials: vec![material],
            instances: vec![LoadedSceneInstanceData {
                mesh,
                materials: vec![material],
                transform: glam::Mat4::IDENTITY,
                name: "instance".to_string(),
            }],
        }
    }

    #[test]
    fn spawn_scene_asset_creates_independent_runtime_instances() {
        let mesh = mesh_handle();
        let material = material_handle();
        let scene_data = scene_data(mesh, material);
        let mut scene = SceneManager::new();

        let first = scene.spawn_scene_asset(&scene_data);
        let second = scene.spawn_scene_asset(&scene_data);

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_ne!(first[0], second[0]);
        assert_eq!(scene.get_instance(first[0]).unwrap().mesh, mesh);
        assert_eq!(scene.get_instance(second[0]).unwrap().materials, vec![material]);
    }
}
