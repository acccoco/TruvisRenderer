use slotmap::SecondaryMap;

use truvis_gfx::commands::barrier::GfxBarrierMask;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_render_interface::frame_counter::FrameToken;
use truvis_render_interface::pipeline_settings::FrameLabel;
use truvis_scene::components::material::ManagedMaterialParams;
use truvis_scene::guid_new_type::{ManagedMaterialHandle, MaterialHandle};
use truvis_scene::material_manager::{MaterialManager, TextureResolver};
use truvis_scene::scene_manager::{MaterialSlotResolver, SceneManager};

struct MaterialBinding {
    managed_handle: ManagedMaterialHandle,
    params: ManagedMaterialParams,
}

/// Render-side 材质桥接层。
///
/// 它把 CPU scene 的 `MaterialHandle` 同步到 `MaterialManager` 的稳定 GPU slot。
/// 这是 Phase 1 的过渡边界：SceneManager 仍保存 CPU 材质语义，shader 可见材质
/// buffer 和 texture fallback/dirty 策略由这里委托给 `MaterialManager`。
pub struct MaterialBridge {
    material_manager: Option<MaterialManager>,
    bindings: SecondaryMap<MaterialHandle, MaterialBinding>,
}

impl MaterialBridge {
    pub fn new(ctx: GfxResourceCtx<'_>, frame_token: FrameToken) -> Self {
        Self {
            material_manager: Some(MaterialManager::new(ctx, frame_token)),
            bindings: SecondaryMap::new(),
        }
    }

    pub fn begin_frame(&mut self, frame_token: FrameToken) {
        self.material_manager_mut().begin_frame(frame_token);
    }

    pub fn sync_scene_materials(&mut self, scene_manager: &SceneManager) {
        let stale_handles: Vec<MaterialHandle> = self
            .bindings
            .iter()
            .filter_map(|(handle, _)| scene_manager.get_material(handle).is_none().then_some(handle))
            .collect();

        for handle in stale_handles {
            let binding = self.bindings.remove(handle).expect("stale material binding missing");
            let slot = self.material_manager().get_slot_index(binding.managed_handle);
            self.material_manager_mut().unregister(binding.managed_handle);
            log::debug!(
                "MaterialBridge: unregister cpu_handle={:?} managed_handle={:?} slot={:?}; reclaim delayed by FIF",
                handle,
                binding.managed_handle,
                slot
            );
        }

        for (handle, mat) in scene_manager.mat_map().iter() {
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
                log::debug!(
                    "MaterialBridge: register cpu_handle={:?} managed_handle={:?} stable_slot={}",
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
                    "MaterialBridge: update cpu_handle={:?} managed_handle={:?} stable_slot={}; dirty all FIF buffers",
                    handle,
                    managed_handle,
                    slot
                );
            }
        }
    }

    pub fn update_textures(&mut self, texture_resolver: &dyn TextureResolver) {
        self.material_manager_mut().update(texture_resolver);
    }

    pub fn upload(
        &mut self,
        ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        frame_label: FrameLabel,
        texture_resolver: &dyn TextureResolver,
    ) {
        self.material_manager_mut().upload(ctx, cmd, barrier_mask, frame_label, texture_resolver);
    }

    pub fn material_buffer_device_address(&self, frame_label: FrameLabel) -> ash::vk::DeviceAddress {
        self.material_manager().material_buffer_device_address(frame_label)
    }

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
    fn resolve_material_slot(&self, handle: MaterialHandle) -> Option<u32> {
        let binding = self.bindings.get(handle)?;
        let slot = self.material_manager().get_slot_index(binding.managed_handle)?;
        u32::try_from(slot).ok()
    }
}
