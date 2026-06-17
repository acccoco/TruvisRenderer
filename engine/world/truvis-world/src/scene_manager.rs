use slotmap::SlotMap;

use truvis_asset::handle::ModelData;
use truvis_shader_binding::gpu;

use crate::components::instance::Instance;
use crate::guid_new_type::{InstanceHandle, LightHandle};

/// CPU 侧 runtime scene 的所有者。
///
/// `SceneManager` 位于 `World` 的 scene 部分，负责保存 live instance / light 的语义状态。
/// 它只分配 `InstanceHandle` / `LightHandle` 这样的 runtime 身份，不创建 GPU 资源，也不解析
/// mesh、material 或 light 在 shader 中的可见绑定。渲染运行时的 `InstanceBridge` 会在
/// prepare/sync 阶段读取这里的数据，并维护 CPU handle 到 GPU scene slot 的映射。
#[derive(Default)]
pub struct SceneManager {
    /// live instance 存储；slotmap key 是 CPU scene 内部的 runtime 身份。
    all_instances: SlotMap<InstanceHandle, Instance>,
    /// live point light 存储；GPU 侧打包和上传由 render runtime 处理。
    all_point_lights: SlotMap<LightHandle, gpu::light::PointLight>,
    /// live spot light 存储；与 point light 分开保存，避免 CPU 语义层提前引入统一 light class。
    all_spot_lights: SlotMap<LightHandle, gpu::light::SpotLight>,
    /// live area light 存储；矩形单面发光的采样语义由 realtime RT shader 解释。
    all_area_lights: SlotMap<LightHandle, gpu::light::AreaLight>,
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
    /// 该只读视图主要供渲染运行时在 prepare/sync 阶段建立 `InstanceBridge` 状态。调用方不应
    /// 把 map key 理解为 GPU slot；稳定 slot 由 render-side bridge 独立维护。
    #[inline]
    pub fn instance_map(&self) -> &SlotMap<InstanceHandle, Instance> {
        &self.all_instances
    }

    /// 返回全部 live point light。
    ///
    /// `PointLight` 类型来自 shader binding，是 CPU/GPU 共享布局数据；本 manager 只保存
    /// CPU 记录，具体 buffer 上传属于 render runtime。
    #[inline]
    pub fn point_light_map(&self) -> &SlotMap<LightHandle, gpu::light::PointLight> {
        &self.all_point_lights
    }

    /// 返回全部 live spot light。
    ///
    /// `SpotLight` 是 CPU/GPU 共享布局数据；本 manager 只保存 CPU 语义记录，具体
    /// buffer 上传与 shader 可见 count 属于 render runtime。
    #[inline]
    pub fn spot_light_map(&self) -> &SlotMap<LightHandle, gpu::light::SpotLight> {
        &self.all_spot_lights
    }

    /// 返回全部 live area light。
    ///
    /// area light 的矩形半轴和 radiance 在 CPU 侧保持原样，RT shader 在 NEE 阶段解释
    /// 单面采样、PDF 和 visibility 契约。
    #[inline]
    pub fn area_light_map(&self) -> &SlotMap<LightHandle, gpu::light::AreaLight> {
        &self.all_area_lights
    }

    /// 判断 CPU scene 是否没有可同步的 live scene 数据。
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.all_instances.is_empty()
            && self.all_point_lights.is_empty()
            && self.all_spot_lights.is_empty()
            && self.all_area_lights.is_empty()
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

    /// 将 model asset / prefab spawn 为 runtime instances。
    ///
    /// `ModelData` 是 asset 层导入后的 prefab CPU 数据，不持有 live instance
    /// 生命周期。每次调用都会创建一组新的 `InstanceHandle`，因此同一个 model asset 可以被
    /// 多次实例化；后续 GPU slot 绑定由 `InstanceBridge` 根据这些 handle 延迟建立。
    pub fn spawn_model(&mut self, model_data: &ModelData) -> Vec<InstanceHandle> {
        model_data
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
    /// 而是在下一次 render runtime 同步时更新。
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
    /// 更新由 render runtime 的 scene 同步流程处理。
    pub fn register_point_light(&mut self, light: gpu::light::PointLight) -> LightHandle {
        self.all_point_lights.insert(light)
    }

    /// 向 CPU scene 添加一个 live spot light。
    ///
    /// spot light 在 realtime RT 中表示半径固定为 0.5 的 sphere emitter，并额外带 cone
    /// falloff；这里不做角度或方向归一化，调用方和 shader ABI 注释共同约束输入单位。
    pub fn register_spot_light(&mut self, light: gpu::light::SpotLight) -> LightHandle {
        self.all_spot_lights.insert(light)
    }

    /// 向 CPU scene 添加一个 live area light。
    ///
    /// area light 使用 world-space `center + half_u + half_v` 描述矩形；本 manager 不计算
    /// 法线或面积，避免 CPU scene 与 shader 采样路径维护两套几何派生规则。
    pub fn register_area_light(&mut self, light: gpu::light::AreaLight) -> LightHandle {
        self.all_area_lights.insert(light)
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
        self.all_spot_lights.clear();
        self.all_area_lights.clear();
    }
}
