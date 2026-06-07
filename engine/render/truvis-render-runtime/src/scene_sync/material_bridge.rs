use slotmap::SecondaryMap;

use truvis_asset::asset_hub::AssetLoadedEvent;
use truvis_asset::handle::{AssetMaterialHandle, MaterialData};

use crate::scene_sync::{
    material_manager::{GpuMaterialHandle, MaterialManager, RenderMaterialParams},
    scene_bridge::MaterialSlotResolver,
};

impl From<&MaterialData> for RenderMaterialParams {
    fn from(mat: &MaterialData) -> Self {
        Self {
            base_color: mat.base_color,
            emissive: mat.emissive,
            metallic: mat.metallic,
            roughness: mat.roughness,
            opaque: mat.opaque,
            diffuse_texture: mat.diffuse_texture,
            normal_texture: mat.normal_texture,
        }
    }
}

/// Render-side 材质桥接层。
///
/// 它把 `AssetHub` 产出的 `MaterialLoaded` 事件转换为 runtime 私有的 GPU material handle。
/// bridge 只保存 asset handle 到 runtime material handle 的身份映射；稳定 slot、shader
/// 可见材质 buffer 和 texture fallback/dirty 策略由 `RenderRuntime` 直接持有的
/// `MaterialManager` 维护。
pub struct MaterialBridge {
    /// asset material handle 到 runtime 私有 GPU material handle 的桥接表。
    ///
    /// 这里不缓存材质参数或 slot；这些状态由 `MaterialManager` 作为唯一 owner 维护，
    /// 避免 bridge 和 manager 之间出现第二份 render-side material 状态。
    bindings: SecondaryMap<AssetMaterialHandle, GpuMaterialHandle>,
}

/// asset material handle 到稳定 GPU material slot 的组合解析器。
///
/// 该解析器只在 prepare 阶段临时存在，同时借用 bridge 的身份映射和 manager 的
/// stable slot 表，避免 `InstanceBridge` 直接依赖具体 material owner。
pub(crate) struct MaterialBridgeSlotResolver<'a> {
    bridge: &'a MaterialBridge,
    material_manager: &'a MaterialManager,
}

// 创建与初始化
impl MaterialBridge {
    /// 创建材质桥接层。
    pub fn new() -> Self {
        Self {
            bindings: SecondaryMap::new(),
        }
    }

    /// 创建 prepare 阶段使用的材质 slot 解析器。
    pub fn slot_resolver<'a>(&'a self, material_manager: &'a MaterialManager) -> MaterialBridgeSlotResolver<'a> {
        MaterialBridgeSlotResolver {
            bridge: self,
            material_manager,
        }
    }
}

// Asset material 事件同步
impl MaterialBridge {
    /// 消费 `AssetHub` 产出的 material loaded 事件，分配或更新稳定 GPU material slot。
    ///
    /// 正常路径下同一 material handle 只会收到一次 loaded event；若调用侧重复传入事件，
    /// bridge 复用原 GPU material handle，并把参数更新和 dirty 状态交给 `MaterialManager`。
    pub fn apply_material_events(&mut self, events: Vec<AssetLoadedEvent>, material_manager: &mut MaterialManager) {
        for event in events {
            match event {
                AssetLoadedEvent::MaterialLoaded { handle, data } => {
                    self.apply_material_loaded(handle, &data, material_manager);
                }
                other => {
                    // RenderRuntime::dispatch_loaded_asset_events 是事件分流边界；
                    // 如果这里收到非 material 事件，说明 runtime 事件契约被调用侧破坏。
                    unreachable!("Unexpected asset event in MaterialBridge: {:?}", other);
                }
            }
        }
    }

    fn apply_material_loaded(
        &mut self,
        handle: AssetMaterialHandle,
        data: &MaterialData,
        material_manager: &mut MaterialManager,
    ) {
        let params = RenderMaterialParams::from(data);

        if let Some(&gpu_handle) = self.bindings.get(handle) {
            // 重复 loaded event 属于防御路径：asset material handle 保持不变时，复用原
            // GPU material handle，具体参数比较、dirty 和 texture ready 状态由 manager 负责。
            let slot = material_manager.get_slot_index(gpu_handle).expect("updated material must keep its slot");
            material_manager.update_params(gpu_handle, params);
            log::debug!(
                "MaterialBridge: update asset_handle={:?} gpu_handle={:?} stable_slot={}; dirty all FIF buffers",
                handle,
                gpu_handle,
                slot
            );
            return;
        }

        // 新 asset material 进入 render-side 后拿到独立 GPU handle 和稳定 GPU slot。
        // 这个 slot 会被 instance bridge 解析进 RenderData。
        let gpu_handle = material_manager.register(params);
        let slot = material_manager.get_slot_index(gpu_handle).expect("registered material must have a slot");
        self.bindings.insert(handle, gpu_handle);
        log::trace!(
            "MaterialBridge: register asset_handle={:?} gpu_handle={:?} stable_slot={}",
            handle,
            gpu_handle,
            slot
        );
    }
}

// 销毁
impl MaterialBridge {
    /// 清空 asset 到 GPU material handle 的映射。
    pub fn destroy(&mut self) {
        self.bindings.clear();
    }
}

impl MaterialSlotResolver for MaterialBridgeSlotResolver<'_> {
    fn resolve_material_slot(&self, handle: AssetMaterialHandle) -> Option<u32> {
        // resolver 是 InstanceBridge 能看到的唯一 material 接口；找不到 binding 表示
        // CPU scene 仍引用了未加载或已删除的 material，实例应保持 pending。
        let gpu_handle = *self.bridge.bindings.get(handle)?;
        let slot = self.material_manager.get_slot_index(gpu_handle)?;
        u32::try_from(slot).ok()
    }
}
