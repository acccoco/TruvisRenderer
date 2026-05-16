use std::collections::HashMap;

use slotmap::SecondaryMap;

use truvis_render_interface::frame_counter::{FrameCounter, FrameToken};
use truvis_render_interface::render_data::{GpuInstanceSlot, InstanceRenderData, MeshRenderData, RenderData};
use truvis_scene::components::instance::Instance;
use truvis_scene::guid_new_type::InstanceHandle;
use truvis_scene::scene_manager::{MaterialSlotResolver, MeshRenderResolver, SceneManager};
use truvis_shader_binding::gpu;

const MAX_INSTANCE_COUNT: u32 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstanceState {
    Pending,
    Active,
}

struct InstanceBinding {
    slot: GpuInstanceSlot,
    state: InstanceState,
    last_transform: glam::Mat4,
}

struct RetiredSlot {
    slot: GpuInstanceSlot,
    retired_frame_id: u64,
}

/// Render-side runtime instance bridge.
///
/// 它为 `InstanceHandle` 分配生命周期内稳定的 GPU instance slot，并在 mesh/material
/// 都 GPU ready 前保持 pending，避免 draw/TLAS 访问未就绪资源。
pub struct InstanceBridge {
    bindings: SecondaryMap<InstanceHandle, InstanceBinding>,
    free_slots: Vec<GpuInstanceSlot>,
    retired_slots: Vec<RetiredSlot>,
    frame_token: FrameToken,
    revision: u64,
}

impl InstanceBridge {
    pub fn new(frame_token: FrameToken) -> Self {
        let free_slots = (0..MAX_INSTANCE_COUNT).rev().map(GpuInstanceSlot::new).collect();
        Self {
            bindings: SecondaryMap::new(),
            free_slots,
            retired_slots: Vec::new(),
            frame_token,
            revision: 0,
        }
    }

    pub fn begin_frame(&mut self, frame_token: FrameToken) {
        self.frame_token = frame_token;
        self.reclaim_retired_slots();
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn prepare_render_data<'a>(
        &mut self,
        scene_manager: &SceneManager,
        material_slot_resolver: &dyn MaterialSlotResolver,
        mesh_resolver: &'a dyn MeshRenderResolver,
    ) -> RenderData<'a> {
        self.sync_scene_instances(scene_manager, material_slot_resolver, mesh_resolver);

        let mut mesh_handle_to_index = HashMap::new();
        let mut all_meshes: Vec<MeshRenderData<'a>> = Vec::new();
        let mut mesh_geometry_start_indices: Vec<usize> = Vec::new();
        let mut total_geometry_count = 0;
        let mut all_instances: Vec<InstanceRenderData> = Vec::new();

        let mut active_instances = self
            .bindings
            .iter()
            .filter_map(|(handle, binding)| {
                (binding.state == InstanceState::Active)
                    .then(|| scene_manager.get_instance(handle).map(|instance| (handle, binding, instance)))
                    .flatten()
            })
            .collect::<Vec<_>>();
        active_instances.sort_by_key(|(_, binding, _)| binding.slot);

        'active: for (_handle, binding, instance) in active_instances {
            let mesh_index = if let Some(&index) = mesh_handle_to_index.get(&instance.mesh) {
                index
            } else {
                let Some(mesh_render_data) = mesh_resolver.resolve_mesh(instance.mesh) else {
                    continue;
                };
                let index = all_meshes.len();
                mesh_handle_to_index.insert(instance.mesh, index);
                mesh_geometry_start_indices.push(total_geometry_count);
                total_geometry_count += mesh_render_data.geometries.len();
                all_meshes.push(mesh_render_data);
                index
            };

            let mut material_slots = Vec::with_capacity(instance.materials.len());
            for &material in &instance.materials {
                let Some(slot) = material_slot_resolver.resolve_material_slot(material) else {
                    continue 'active;
                };
                material_slots.push(slot);
            }

            all_instances.push(InstanceRenderData {
                instance_slot: binding.slot,
                mesh_index,
                material_slots,
                transform: instance.transform,
            });
        }

        let all_point_lights: Vec<gpu::PointLight> =
            scene_manager.point_light_map().iter().map(|(_, light)| *light).collect();

        RenderData {
            all_instances,
            all_meshes,
            all_point_lights,
            mesh_geometry_start_indices,
            total_geometry_count,
        }
    }

    fn sync_scene_instances(
        &mut self,
        scene_manager: &SceneManager,
        material_slot_resolver: &dyn MaterialSlotResolver,
        mesh_resolver: &dyn MeshRenderResolver,
    ) {
        self.retire_stale_instances(scene_manager);

        for (handle, instance) in scene_manager.instance_map().iter() {
            if !self.bindings.contains_key(handle) {
                self.register_instance(handle, instance);
            }

            let ready = Self::dependencies_ready(instance, material_slot_resolver, mesh_resolver);
            let binding = self.bindings.get_mut(handle).expect("instance binding missing after register");

            if binding.last_transform != instance.transform {
                binding.last_transform = instance.transform;
                if binding.state == InstanceState::Active {
                    self.revision = self.revision.saturating_add(1);
                    log::debug!(
                        "InstanceBridge: transform dirty handle={:?} stable_slot={}",
                        handle,
                        binding.slot.as_u32()
                    );
                }
            }

            match (binding.state, ready) {
                (InstanceState::Pending, true) => {
                    binding.state = InstanceState::Active;
                    self.revision = self.revision.saturating_add(1);
                    log::debug!("InstanceBridge: activate handle={:?} stable_slot={}", handle, binding.slot.as_u32());
                }
                (InstanceState::Active, false) => {
                    binding.state = InstanceState::Pending;
                    self.revision = self.revision.saturating_add(1);
                    log::debug!("InstanceBridge: deactivate handle={:?} stable_slot={}", handle, binding.slot.as_u32());
                }
                _ => {}
            }
        }
    }

    fn register_instance(&mut self, handle: InstanceHandle, instance: &Instance) {
        let slot = self.free_slots.pop().expect("InstanceBridge: GPU instance slots exhausted");
        self.bindings.insert(
            handle,
            InstanceBinding {
                slot,
                state: InstanceState::Pending,
                last_transform: instance.transform,
            },
        );
        log::debug!("InstanceBridge: register handle={:?} stable_slot={}", handle, slot.as_u32());
    }

    fn retire_stale_instances(&mut self, scene_manager: &SceneManager) {
        let stale_handles = self
            .bindings
            .iter()
            .filter_map(|(handle, _)| scene_manager.get_instance(handle).is_none().then_some(handle))
            .collect::<Vec<_>>();

        for handle in stale_handles {
            let binding = self.bindings.remove(handle).expect("stale instance binding missing");
            if binding.state == InstanceState::Active {
                self.revision = self.revision.saturating_add(1);
            }
            self.retired_slots.push(RetiredSlot {
                slot: binding.slot,
                retired_frame_id: self.frame_token.frame_id(),
            });
            log::debug!(
                "InstanceBridge: retire handle={:?} stable_slot={}; reclaim delayed by FIF",
                handle,
                binding.slot.as_u32()
            );
        }
    }

    fn reclaim_retired_slots(&mut self) {
        let current_frame_id = self.frame_token.frame_id();
        let fif_count = FrameCounter::fif_count() as u64;
        let mut retained = Vec::new();

        for retired in self.retired_slots.drain(..) {
            if current_frame_id.saturating_sub(retired.retired_frame_id) >= fif_count {
                log::debug!("InstanceBridge: reclaimed stable_slot={}", retired.slot.as_u32());
                self.free_slots.push(retired.slot);
            } else {
                retained.push(retired);
            }
        }

        self.retired_slots = retained;
    }

    fn dependencies_ready(
        instance: &Instance,
        material_slot_resolver: &dyn MaterialSlotResolver,
        mesh_resolver: &dyn MeshRenderResolver,
    ) -> bool {
        mesh_resolver.is_mesh_ready(instance.mesh)
            && instance.materials.iter().all(|&material| material_slot_resolver.is_material_ready(material))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use slotmap::SlotMap;
    use truvis_asset::handle::{AssetMaterialHandle, AssetMeshHandle};
    use truvis_render_interface::frame_counter::FrameCounter;

    use super::*;

    #[derive(Default)]
    struct FakeMaterialResolver {
        ready: HashSet<AssetMaterialHandle>,
    }

    impl MaterialSlotResolver for FakeMaterialResolver {
        fn resolve_material_slot(&self, handle: AssetMaterialHandle) -> Option<u32> {
            self.ready.contains(&handle).then_some(7)
        }
    }

    #[derive(Default)]
    struct FakeMeshResolver {
        ready: HashSet<AssetMeshHandle>,
    }

    impl MeshRenderResolver for FakeMeshResolver {
        fn is_mesh_ready(&self, handle: AssetMeshHandle) -> bool {
            self.ready.contains(&handle)
        }

        fn resolve_mesh(&self, _handle: AssetMeshHandle) -> Option<MeshRenderData<'_>> {
            None
        }
    }

    fn frame_token(frame_id: u64) -> FrameToken {
        FrameCounter::new(frame_id, 60.0).frame_token()
    }

    fn mesh_handle() -> AssetMeshHandle {
        SlotMap::<AssetMeshHandle, ()>::with_key().insert(())
    }

    fn material_handle() -> AssetMaterialHandle {
        SlotMap::<AssetMaterialHandle, ()>::with_key().insert(())
    }

    fn instance(mesh: AssetMeshHandle, material: AssetMaterialHandle, transform: glam::Mat4) -> Instance {
        Instance {
            mesh,
            materials: vec![material],
            transform,
        }
    }

    fn active_slots(bridge: &InstanceBridge) -> Vec<u32> {
        let mut slots = bridge
            .bindings
            .iter()
            .filter_map(|(_, binding)| (binding.state == InstanceState::Active).then_some(binding.slot.as_u32()))
            .collect::<Vec<_>>();
        slots.sort();
        slots
    }

    #[test]
    fn instance_slot_is_stable_when_pending_instance_becomes_active() {
        let mesh = mesh_handle();
        let material = material_handle();
        let mut scene = SceneManager::new();
        let handle = scene.register_instance(instance(mesh, material, glam::Mat4::IDENTITY));
        let mut material_resolver = FakeMaterialResolver::default();
        material_resolver.ready.insert(material);
        let mut mesh_resolver = FakeMeshResolver::default();
        let mut bridge = InstanceBridge::new(frame_token(1));

        bridge.sync_scene_instances(&scene, &material_resolver, &mesh_resolver);
        let slot = bridge.bindings.get(handle).unwrap().slot;
        assert_eq!(active_slots(&bridge), Vec::<u32>::new());

        mesh_resolver.ready.insert(mesh);
        bridge.sync_scene_instances(&scene, &material_resolver, &mesh_resolver);

        assert_eq!(bridge.bindings.get(handle).unwrap().slot, slot);
        assert_eq!(active_slots(&bridge), vec![slot.as_u32()]);
    }

    #[test]
    fn active_instances_are_reported_in_slot_order() {
        let mesh_a = mesh_handle();
        let mesh_b = mesh_handle();
        let material = material_handle();
        let mut scene = SceneManager::new();
        scene.register_instance(instance(mesh_a, material, glam::Mat4::IDENTITY));
        scene.register_instance(instance(mesh_b, material, glam::Mat4::IDENTITY));
        let mut material_resolver = FakeMaterialResolver::default();
        material_resolver.ready.insert(material);
        let mut mesh_resolver = FakeMeshResolver::default();
        mesh_resolver.ready.insert(mesh_a);
        mesh_resolver.ready.insert(mesh_b);
        let mut bridge = InstanceBridge::new(frame_token(1));

        bridge.sync_scene_instances(&scene, &material_resolver, &mesh_resolver);

        assert_eq!(active_slots(&bridge), vec![0, 1]);
    }

    #[test]
    fn retired_slot_is_not_reused_until_fif_delay_passes() {
        let mesh = mesh_handle();
        let material = material_handle();
        let mut scene = SceneManager::new();
        let first = scene.register_instance(instance(mesh, material, glam::Mat4::IDENTITY));
        let mut material_resolver = FakeMaterialResolver::default();
        material_resolver.ready.insert(material);
        let mut mesh_resolver = FakeMeshResolver::default();
        mesh_resolver.ready.insert(mesh);
        let mut bridge = InstanceBridge::new(frame_token(1));

        bridge.sync_scene_instances(&scene, &material_resolver, &mesh_resolver);
        let first_slot = bridge.bindings.get(first).unwrap().slot;
        scene.remove_instance(first);
        bridge.sync_scene_instances(&scene, &material_resolver, &mesh_resolver);

        let second = scene.register_instance(instance(mesh, material, glam::Mat4::IDENTITY));
        bridge.sync_scene_instances(&scene, &material_resolver, &mesh_resolver);
        let second_slot = bridge.bindings.get(second).unwrap().slot;
        assert_ne!(second_slot, first_slot);

        scene.remove_instance(second);
        bridge.sync_scene_instances(&scene, &material_resolver, &mesh_resolver);
        bridge.begin_frame(frame_token(1 + FrameCounter::fif_count() as u64));
        let third = scene.register_instance(instance(mesh, material, glam::Mat4::IDENTITY));
        bridge.sync_scene_instances(&scene, &material_resolver, &mesh_resolver);

        let third_slot = bridge.bindings.get(third).unwrap().slot;
        assert!(third_slot == first_slot || third_slot == second_slot);
    }

    #[test]
    fn active_transform_change_advances_revision_and_keeps_slot() {
        let mesh = mesh_handle();
        let material = material_handle();
        let mut scene = SceneManager::new();
        let handle = scene.register_instance(instance(mesh, material, glam::Mat4::IDENTITY));
        let mut material_resolver = FakeMaterialResolver::default();
        material_resolver.ready.insert(material);
        let mut mesh_resolver = FakeMeshResolver::default();
        mesh_resolver.ready.insert(mesh);
        let mut bridge = InstanceBridge::new(frame_token(1));
        bridge.sync_scene_instances(&scene, &material_resolver, &mesh_resolver);
        let slot = bridge.bindings.get(handle).unwrap().slot;
        let revision = bridge.revision();

        scene.update_instance_transform(handle, glam::Mat4::from_translation(glam::Vec3::X));
        bridge.sync_scene_instances(&scene, &material_resolver, &mesh_resolver);

        assert_eq!(bridge.bindings.get(handle).unwrap().slot, slot);
        assert!(bridge.revision() > revision);
    }
}
