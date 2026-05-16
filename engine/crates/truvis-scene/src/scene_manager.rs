use slotmap::SlotMap;

use truvis_asset::handle::LoadedSceneData;
use truvis_shader_binding::gpu;

use crate::components::instance::Instance;
use crate::guid_new_type::{InstanceHandle, LightHandle};

/// 在 CPU 侧管理场景数据
#[derive(Default)]
pub struct SceneManager {
    all_instances: SlotMap<InstanceHandle, Instance>,
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
    pub fn instance_map(&self) -> &SlotMap<InstanceHandle, Instance> {
        &self.all_instances
    }
    #[inline]
    pub fn point_light_map(&self) -> &SlotMap<LightHandle, gpu::PointLight> {
        &self.all_point_lights
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.all_instances.is_empty() && self.all_point_lights.is_empty()
    }
}
// 工具函数
impl SceneManager {
    #[inline]
    pub fn get_instance(&self, handle: InstanceHandle) -> Option<&Instance> {
        self.all_instances.get(handle)
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
    pub fn destroy(mut self) {
        self.destroy_mut();
    }
    pub fn destroy_mut(&mut self) {
        self.all_instances.clear();
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
