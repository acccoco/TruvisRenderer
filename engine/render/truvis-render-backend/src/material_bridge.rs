use slotmap::SecondaryMap;

use truvis_asset::asset_hub::AssetHub;
use truvis_asset::handle::AssetMaterialHandle;
use truvis_gfx::commands::barrier::GfxBarrierMask;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_render_interface::frame_counter::FrameToken;
use truvis_render_interface::pipeline_settings::FrameLabel;

use crate::material_manager::{ManagedMaterialHandle, ManagedMaterialParams, MaterialManager, TextureResolver};
use crate::scene_bridge::MaterialSlotResolver;

struct MaterialBinding {
    managed_handle: ManagedMaterialHandle,
    params: ManagedMaterialParams,
}

/// Render-side 材质桥接层。
///
/// 它把 `AssetHub` 中的 `AssetMaterialHandle` 同步到 `MaterialManager` 的稳定 GPU slot。
/// shader 可见材质 buffer 和 texture fallback/dirty 策略由这里委托给 `MaterialManager`。
/// 因此 CPU scene 只需要保存 asset handle，render pass 看到的始终是稳定 slot 和 GPU buffer。
pub struct MaterialBridge {
    material_manager: Option<MaterialManager>,
    bindings: SecondaryMap<AssetMaterialHandle, MaterialBinding>,
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

    /// 以 `AssetHub` 为 CPU 事实来源，同步 asset material 到稳定 GPU material slot。
    ///
    /// 新增 material 会注册到 `MaterialManager`，参数变化会标记 dirty，删除则交给 manager
    /// 做 FIF 延迟回收。CPU scene 仍只保存 `AssetMaterialHandle`，不感知 managed handle。
    pub fn sync_asset_materials(&mut self, asset_hub: &AssetHub) {
        // AssetHub 是 CPU 资产事实来源；bridge 每帧以它为准同步新增、修改和删除。
        // 删除不会立刻复用 slot，真正的 FIF 延迟回收由 MaterialManager 负责。
        let stale_handles: Vec<AssetMaterialHandle> = self
            .bindings
            .iter()
            .filter_map(|(handle, _)| asset_hub.get_material_data(handle).is_none().then_some(handle))
            .collect();

        for handle in stale_handles {
            let binding = self.bindings.remove(handle).expect("stale material binding missing");
            let slot = self.material_manager().get_slot_index(binding.managed_handle);
            self.material_manager_mut().unregister(binding.managed_handle);
            log::debug!(
                "MaterialBridge: unregister asset_handle={:?} managed_handle={:?} slot={:?}; reclaim delayed by FIF",
                handle,
                binding.managed_handle,
                slot
            );
        }

        for (handle, mat) in asset_hub.iter_materials() {
            let params = ManagedMaterialParams::from(mat);
            let mut changed_managed_handle = None;

            if let Some(binding) = self.bindings.get_mut(handle) {
                if binding.params != params {
                    binding.params = params.clone();
                    changed_managed_handle = Some(binding.managed_handle);
                }
            } else {
                let managed_handle = self.material_manager_mut().register(params.clone());
                let slot = self
                    .material_manager()
                    .get_slot_index(managed_handle)
                    .expect("registered material must have a slot");
                self.bindings.insert(handle, MaterialBinding { managed_handle, params });
                log::trace!(
                    "MaterialBridge: register asset_handle={:?} managed_handle={:?} stable_slot={}",
                    handle,
                    managed_handle,
                    slot
                );
                continue;
            }

            if let Some(managed_handle) = changed_managed_handle {
                let slot = self
                    .material_manager()
                    .get_slot_index(managed_handle)
                    .expect("updated material must keep its slot");
                self.material_manager_mut().update_params(managed_handle, params);
                log::debug!(
                    "MaterialBridge: update asset_handle={:?} managed_handle={:?} stable_slot={}; dirty all FIF buffers",
                    handle,
                    managed_handle,
                    slot
                );
            }
        }
    }

    /// 根据纹理上传器的 ready 状态更新材质 dirty 标记。
    ///
    /// 当材质引用的贴图从 fallback/null 变成真实 SRV 时，MaterialManager 会把所有 FIF buffer
    /// 标记为 dirty，让每个在飞帧对应的 material buffer 都逐步更新。
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

    /// 销毁 material GPU buffer 并清空 asset 到 managed material 的映射。
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
        let binding = self.bindings.get(handle)?;
        let slot = self.material_manager().get_slot_index(binding.managed_handle)?;
        u32::try_from(slot).ok()
    }
}
