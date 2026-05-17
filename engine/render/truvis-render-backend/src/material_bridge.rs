use slotmap::SecondaryMap;

use truvis_asset::asset_hub::AssetLoadedEvent;
use truvis_asset::handle::{AssetMaterialHandle, MaterialData};
use truvis_gfx::commands::barrier::GfxBarrierMask;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_render_interface::frame_counter::FrameToken;
use truvis_render_interface::pipeline_settings::FrameLabel;

use crate::scene_bridge::MaterialSlotResolver;
use crate::{
    material_manager::{GpuMaterialHandle, MaterialManager, RenderMaterialParams},
    texture_resolver::TextureResolver,
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
/// 它把 `AssetHub` 产出的 `MaterialLoaded` 事件转换为 backend 私有的 GPU material handle。
/// 稳定 slot、shader 可见材质 buffer 和 texture fallback/dirty 策略由这里委托给 `MaterialManager`。
/// 因此 CPU scene 只需要保存 asset handle，render pass 看到的始终是稳定 slot 和 GPU buffer。
pub struct MaterialBridge {
    material_manager: Option<MaterialManager>,
    /// asset material handle 到 backend 私有 GPU material handle 的桥接表。
    ///
    /// 这里不缓存材质参数或 slot；这些状态由 `MaterialManager` 作为唯一 owner 维护，
    /// 避免 bridge 和 manager 之间出现第二份 render-side material 状态。
    bindings: SecondaryMap<AssetMaterialHandle, GpuMaterialHandle>,
}

impl MaterialBridge {
    /// 创建材质桥接层与底层 `MaterialManager`。
    ///
    /// `frame_token` 用于 dirty/FIF 回收计时，必须在每帧通过 `begin_frame` 保持同步。
    pub fn new(ctx: GfxResourceCtx<'_>, frame_token: FrameToken) -> Self {
        Self {
            material_manager: Some(MaterialManager::new(ctx, frame_token)),
            bindings: SecondaryMap::new(),
        }
    }

    /// 帧开始时同步 frame token，推进 MaterialManager 的延迟回收时间基准。
    pub fn begin_frame(&mut self, frame_token: FrameToken) {
        self.material_manager_mut().begin_frame(frame_token);
    }

    /// 消费 `AssetHub` 产出的 material loaded 事件，分配或更新稳定 GPU material slot。
    ///
    /// 正常路径下同一 material handle 只会收到一次 loaded event；若调用侧重复传入事件，
    /// bridge 复用原 GPU material handle，并把参数更新和 dirty 状态交给 `MaterialManager`。
    pub fn apply_material_events(&mut self, events: Vec<AssetLoadedEvent>) {
        for event in events {
            match event {
                AssetLoadedEvent::MaterialLoaded { handle, data } => {
                    self.apply_material_loaded(handle, &data);
                }
                other => {
                    // AssetUploadStage 是事件分流边界；如果这里收到非 material 事件，
                    // 说明 backend prepare 流程的分层契约被调用侧破坏。
                    unreachable!("Unexpected asset event in MaterialBridge: {:?}", other);
                }
            }
        }
    }

    fn apply_material_loaded(&mut self, handle: AssetMaterialHandle, data: &MaterialData) {
        let params = RenderMaterialParams::from(data);

        if let Some(&gpu_handle) = self.bindings.get(handle) {
            // 重复 loaded event 属于防御路径：asset material handle 保持不变时，复用原
            // GPU material handle，具体参数比较、dirty 和 texture ready 状态由 manager 负责。
            let material_manager = self.material_manager_mut();
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
        let material_manager = self.material_manager_mut();
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

    /// 根据纹理上传器的 ready 状态更新材质 dirty 标记。
    ///
    /// 当材质引用的贴图从 fallback/null 变成真实 SRV 时，MaterialManager 会把所有 FIF buffer
    /// 标记为 dirty，让每个在flight-frame对应的 material buffer 都逐步更新。
    pub fn update_textures(&mut self, texture_resolver: &dyn TextureResolver) {
        // texture ready 状态属于纹理上传器，material bridge 只把 resolver 注入给 manager，
        // 由 manager 决定哪些材质需要从 fallback/null binding 切换到真实 SRV。
        self.material_manager_mut().update(texture_resolver);
    }

    /// 上传当前 frame label 的 dirty material slot 到 GPU buffer。
    ///
    /// `barrier_mask` 由 backend prepare 命令统一提供，保证 material buffer copy 对后续 shader
    /// 读取可见；非当前 FIF buffer 的 dirty 状态会保留到对应 frame label 再处理。
    pub fn upload(
        &mut self,
        ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        frame_label: FrameLabel,
        texture_resolver: &dyn TextureResolver,
    ) {
        // upload 只写当前 frame label 对应的材质 buffer，其他 FIF buffer 的 dirty 状态保留到
        // 它们各自成为当前帧时再处理，避免跨帧 buffer 被 CPU/GPU 同时改写。
        self.material_manager_mut().upload(ctx, cmd, barrier_mask, frame_label, texture_resolver);
    }

    /// 当前 frame label 的 material buffer device address。
    ///
    /// `GpuScene` 会把它写入 scene root buffer，shader 通过该地址索引稳定 material slot。
    pub fn material_buffer_device_address(&self, frame_label: FrameLabel) -> ash::vk::DeviceAddress {
        self.material_manager().material_buffer_device_address(frame_label)
    }

    /// 销毁 material GPU buffer 并清空 asset 到 GPU material handle 的映射。
    pub fn destroy(&mut self, ctx: GfxResourceCtx<'_>) {
        if let Some(material_manager) = self.material_manager.take() {
            material_manager.destroy(ctx);
        }
        self.bindings.clear();
    }

    fn material_manager(&self) -> &MaterialManager {
        self.material_manager.as_ref().expect("MaterialBridge used after shutdown")
    }

    fn material_manager_mut(&mut self) -> &mut MaterialManager {
        self.material_manager.as_mut().expect("MaterialBridge used after shutdown")
    }
}

impl MaterialSlotResolver for MaterialBridge {
    fn resolve_material_slot(&self, handle: AssetMaterialHandle) -> Option<u32> {
        // resolver 是 InstanceBridge 能看到的唯一 material 接口；找不到 binding 表示
        // CPU scene 仍引用了未加载或已删除的 material，实例应保持 pending。
        let gpu_handle = *self.bindings.get(handle)?;
        let slot = self.material_manager().get_slot_index(gpu_handle)?;
        u32::try_from(slot).ok()
    }
}
