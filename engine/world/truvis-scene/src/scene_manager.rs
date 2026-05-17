use slotmap::SlotMap;

use truvis_asset::handle::SceneData;
use truvis_shader_binding::gpu;

use crate::components::instance::Instance;
use crate::guid_new_type::{InstanceHandle, LightHandle};

/// CPU 侧 runtime scene 的所有者。
///
/// `SceneManager` 位于 `World` 的 scene 部分，负责保存 live instance / light 的语义状态。
/// 它只分配 `InstanceHandle` / `LightHandle` 这样的 runtime 身份，不创建 GPU 资源，也不解析
/// mesh、material 或 light 在 shader 中的可见绑定。渲染后端的 `InstanceBridge` 会在
/// prepare/sync 阶段读取这里的数据，并维护 CPU handle 到 GPU scene slot 的映射。
#[derive(Default)]
pub struct SceneManager {
    /// live instance 存储；slotmap key 是 CPU scene 内部的 runtime 身份。
    all_instances: SlotMap<InstanceHandle, Instance>,
    /// live point light 存储；GPU 侧打包和上传由 render backend 处理。
    all_point_lights: SlotMap<LightHandle, gpu::PointLight>,
}
// 创建与初始化
impl SceneManager {
    /// 创建空的 CPU scene manager。
    pub fn new() -> Self {
        Self::default()
    }
}
// 访问器
impl SceneManager {
    /// 返回全部 live instance。
    ///
    /// 该只读视图主要供渲染后端在 prepare/sync 阶段建立 `InstanceBridge` 状态。调用方不应
    /// 把 map key 理解为 GPU slot；稳定 slot 由 render-side bridge 独立维护。
    #[inline]
    pub fn instance_map(&self) -> &SlotMap<InstanceHandle, Instance> {
        &self.all_instances
    }

    /// 返回全部 live point light。
    ///
    /// `PointLight` 类型来自 shader binding，是 CPU/GPU 共享布局数据；本 manager 只保存
    /// CPU 记录，具体 buffer 上传属于 render backend。
    #[inline]
    pub fn point_light_map(&self) -> &SlotMap<LightHandle, gpu::PointLight> {
        &self.all_point_lights
    }

    /// 判断 CPU scene 是否没有可同步的 live scene 数据。
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.all_instances.is_empty() && self.all_point_lights.is_empty()
    }
}
// 工具函数
impl SceneManager {
    /// 按 CPU runtime handle 查询 live instance。
    #[inline]
    pub fn get_instance(&self, handle: InstanceHandle) -> Option<&Instance> {
        self.all_instances.get(handle)
    }

    /// 向 CPU scene 添加一个 live instance，并返回它的 runtime 身份。
    ///
    /// 注册只改变 CPU 语义状态；mesh/material asset 是否已经 GPU-ready 由 render-side
    /// bridge 在同步时检查。
    pub fn register_instance(&mut self, instance: Instance) -> InstanceHandle {
        self.all_instances.insert(instance)
    }

    /// 将 scene asset / prefab spawn 为 runtime instances。
    ///
    /// `SceneData` 是 asset 层导入后的 prefab CPU 数据，不持有 live instance
    /// 生命周期。每次调用都会创建一组新的 `InstanceHandle`，因此同一个 scene asset 可以被
    /// 多次实例化；后续 GPU slot 绑定由 `InstanceBridge` 根据这些 handle 延迟建立。
    pub fn spawn_scene_asset(&mut self, scene_data: &SceneData) -> Vec<InstanceHandle> {
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

    /// 从 CPU scene 移除 live instance。
    ///
    /// 返回的 instance 数据只代表 CPU 记录。已建立的 GPU-side 映射会在后续 prepare/sync
    /// 阶段被 `InstanceBridge` 识别为 stale 并回收。
    pub fn remove_instance(&mut self, handle: InstanceHandle) -> Option<Instance> {
        self.all_instances.remove(handle)
    }

    /// 更新 live instance 的 CPU world transform。
    ///
    /// 返回 `false` 表示 handle 已失效或不属于当前 scene。GPU scene 数据不会在这里直接写入，
    /// 而是在下一次 render backend 同步时更新。
    pub fn update_instance_transform(&mut self, handle: InstanceHandle, transform: glam::Mat4) -> bool {
        let Some(instance) = self.all_instances.get_mut(handle) else {
            return false;
        };
        instance.transform = transform;
        true
    }

    /// 向 CPU scene 添加一个 live point light。
    ///
    /// 光源使用 shader binding 中的共享布局类型，但这里仍只负责 CPU 侧生命周期；GPU buffer
    /// 更新由 render backend 的 scene 同步流程处理。
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
    /// 消耗 manager 并释放其 CPU scene 记录。
    pub fn destroy(mut self) {
        self.destroy_mut();
    }

    /// 清空 CPU scene 记录，供拥有者按既有 destroy 顺序显式释放。
    pub fn destroy_mut(&mut self) {
        self.all_instances.clear();
        self.all_point_lights.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use slotmap::SlotMap;
    use truvis_asset::handle::{AssetMaterialHandle, AssetMeshHandle, SceneData, SceneInstanceData};

    use super::*;

    fn mesh_handle() -> AssetMeshHandle {
        SlotMap::<AssetMeshHandle, ()>::with_key().insert(())
    }

    fn material_handle() -> AssetMaterialHandle {
        SlotMap::<AssetMaterialHandle, ()>::with_key().insert(())
    }

    fn scene_data(mesh: AssetMeshHandle, material: AssetMaterialHandle) -> SceneData {
        SceneData {
            source_path: PathBuf::from("assets/model.fbx"),
            name: "model.fbx".to_string(),
            meshes: vec![mesh],
            materials: vec![material],
            instances: vec![SceneInstanceData {
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
