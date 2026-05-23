use std::collections::HashMap;

use slotmap::SecondaryMap;

use truvis_asset::handle::{AssetMaterialHandle, AssetMeshHandle};
use truvis_render_foundation::frame_counter::{FrameCounter, FrameToken};
use truvis_shader_binding::gpu;
use truvis_world::components::instance::Instance;
use truvis_world::guid_new_type::InstanceHandle;
use truvis_world::scene_manager::SceneManager;

use crate::render_scene::render_data::{GpuInstanceSlot, InstanceRenderData, MeshRenderData, RenderData};
use crate::scene_bridge::{MaterialSlotResolver, MeshRenderResolver};

const MAX_INSTANCE_COUNT: u32 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstanceState {
    /// 已分配稳定 slot，但 mesh/material 依赖尚未全部 GPU-ready。
    Pending,
    /// 可进入 `RenderData`，对应 slot 会被写入 instance buffer 和 TLAS custom index。
    Active,
}

/// 单个 runtime instance 在 render-side 的稳定绑定。
///
/// `last_transform` 用于检测 active 实例 transform 变化，从而推进 TLAS/instance buffer revision。
struct InstanceBinding {
    slot: GpuInstanceSlot,
    state: InstanceState,
    last_transform: glam::Mat4,
}

/// 已删除 instance 的 slot 延迟回收记录。
///
/// slot 不能立即复用，因为旧 command buffer 中可能仍通过 instance index 读取 GPU buffer。
struct RetiredSlot {
    slot: GpuInstanceSlot,
    retired_frame_id: u64,
}

/// 同步 raycast 使用的 CPU 反查记录。
///
/// GPU hit 只返回稳定 instance slot 与 submesh index；该快照在 prepare 阶段生成，
/// 保证 after_prepare 中的同步查询能把 GPU world 结果还原成 CPU scene/asset handle。
#[derive(Clone)]
pub(crate) struct RayCastInstanceRecord {
    pub(crate) instance: InstanceHandle,
    pub(crate) mesh: AssetMeshHandle,
    pub(crate) materials: Vec<AssetMaterialHandle>,
}

/// Render-side runtime instance bridge.
///
/// 它为 `InstanceHandle` 分配生命周期内稳定的 GPU instance slot，并在 mesh/material
/// 都 GPU ready 前保持 pending，避免 draw/TLAS 访问未就绪资源。
/// bridge 是 CPU `SceneManager` 与 runtime 私有 `RenderData` 之间的翻译层：SceneManager
/// 保存语义实例，GpuScene 只接收按稳定 slot 排序、依赖已就绪的渲染快照。
pub struct InstanceBridge {
    bindings: SecondaryMap<InstanceHandle, InstanceBinding>,
    free_slots: Vec<GpuInstanceSlot>,
    retired_slots: Vec<RetiredSlot>,
    frame_token: FrameToken,
    revision: u64,
    ray_cast_records: Vec<Option<RayCastInstanceRecord>>,
}

impl InstanceBridge {
    /// 创建 instance bridge，并预分配稳定 GPU instance slot 池。
    ///
    /// slot 数量当前与 `GpuScene` instance buffer 容量保持一致；耗尽表示 CPU scene 中可渲染实例
    /// 已超过 runtime 当前固定容量。
    pub fn new(frame_token: FrameToken) -> Self {
        let free_slots = (0..MAX_INSTANCE_COUNT).rev().map(GpuInstanceSlot::new).collect();
        Self {
            bindings: SecondaryMap::new(),
            free_slots,
            retired_slots: Vec::new(),
            frame_token,
            revision: 0,
            ray_cast_records: vec![None; MAX_INSTANCE_COUNT as usize],
        }
    }

    /// 帧开始时推进 frame token，并回收已经跨过 FIF 窗口的 retired slot。
    pub fn begin_frame(&mut self, frame_token: FrameToken) {
        // slot 回收以 frame id 为准推进；每帧开始时回收已经跨过 FIF 窗口的旧 slot。
        self.frame_token = frame_token;
        self.reclaim_retired_slots();
    }

    /// 返回影响 TLAS/instance buffer 的 instance-side revision。
    ///
    /// 实例增删、ready 状态变化和 active transform 变化都会推进该值。
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// 读取当前 prepare 快照中的 raycast 反查记录。
    pub(crate) fn ray_cast_record(&self, instance_slot: u32) -> Option<&RayCastInstanceRecord> {
        self.ray_cast_records.get(instance_slot as usize)?.as_ref()
    }

    /// 从 CPU `SceneManager` 构建本帧可渲染的 `RenderData` 快照。
    ///
    /// 该阶段会先同步 instance 生命周期与依赖 ready 状态；只有 mesh/material 都能被 resolver
    /// 解析到 GPU 数据的实例才进入 active 列表。输出按稳定 slot 排序，保证 raster draw、
    /// TLAS custom index 和 GPU instance buffer 共享同一套 instance slot 语义。
    pub fn prepare_render_data<'a>(
        &mut self,
        scene_manager: &SceneManager,
        material_slot_resolver: &dyn MaterialSlotResolver,
        mesh_resolver: &'a dyn MeshRenderResolver,
    ) -> RenderData<'a> {
        self.sync_scene_instances(scene_manager, material_slot_resolver, mesh_resolver);

        // RenderData 是提交给 GpuScene 的只读快照。这里按稳定 slot 排序，保证 raster draw、
        // TLAS custom index 和 GPU instance buffer 使用同一套 instance slot 语义。
        let mut mesh_handle_to_index = HashMap::new();
        let mut all_meshes: Vec<MeshRenderData<'a>> = Vec::new();
        let mut mesh_geometry_start_indices: Vec<usize> = Vec::new();
        let mut total_geometry_count = 0;
        let mut all_instances: Vec<InstanceRenderData> = Vec::new();
        self.ray_cast_records.fill(None);

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

        'active: for (handle, binding, instance) in active_instances {
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
            self.ray_cast_records[binding.slot.as_usize()] = Some(RayCastInstanceRecord {
                instance: handle,
                mesh: instance.mesh,
                materials: instance.materials.clone(),
            });
        }

        let all_point_lights: Vec<gpu::PointLight> =
            scene_manager.point_light_map().iter().map(|(_, light)| *light).collect();

        RenderData {
            all_instances,
            all_meshes,
            all_point_lights,
            mesh_geometry_start_indices,
        }
    }

    fn sync_scene_instances(
        &mut self,
        scene_manager: &SceneManager,
        material_slot_resolver: &dyn MaterialSlotResolver,
        mesh_resolver: &dyn MeshRenderResolver,
    ) {
        // 先处理 CPU scene 中已经不存在的实例，避免后续 active 列表继续输出 stale slot。
        self.retire_stale_instances(scene_manager);

        for (handle, instance) in scene_manager.instance_map().iter() {
            if !self.bindings.contains_key(handle) {
                self.register_instance(handle, instance);
            }

            // ready gate 由 material/mesh resolver 共同决定。bridge 不直接访问 uploader/manager
            // 内部缓存，只依赖窄接口判断这个实例是否可以进入本帧 render data。
            let ready = Self::dependencies_ready(instance, material_slot_resolver, mesh_resolver);
            let binding = self.bindings.get_mut(handle).expect("instance binding missing after register");

            if binding.last_transform != instance.transform {
                binding.last_transform = instance.transform;
                if binding.state == InstanceState::Active {
                    // transform 变化会影响 instance buffer 与 TLAS transform，
                    // revision 用来让 GpuScene 知道当前帧需要重建 TLAS。
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
                    // mesh/material 都 ready 后才激活，避免 draw/TLAS 使用空 BLAS 或无效 material slot。
                    binding.state = InstanceState::Active;
                    self.revision = self.revision.saturating_add(1);
                    log::trace!("InstanceBridge: activate handle={:?} stable_slot={}", handle, binding.slot.as_u32());
                }
                (InstanceState::Active, false) => {
                    // asset 重新加载或材质被移除时，已激活实例会退回 pending，
                    // 直到 resolver 再次提供完整 GPU 数据。
                    binding.state = InstanceState::Pending;
                    self.revision = self.revision.saturating_add(1);
                    log::trace!("InstanceBridge: deactivate handle={:?} stable_slot={}", handle, binding.slot.as_u32());
                }
                _ => {}
            }
        }
    }

    fn register_instance(&mut self, handle: InstanceHandle, instance: &Instance) {
        // 新实例先拿到稳定 slot，但初始状态保持 pending；ready gate 由 resolver 决定。
        let slot = self.free_slots.pop().expect("InstanceBridge: GPU instance slots exhausted");
        self.bindings.insert(
            handle,
            InstanceBinding {
                slot,
                state: InstanceState::Pending,
                last_transform: instance.transform,
            },
        );
        log::trace!("InstanceBridge: register handle={:?} stable_slot={}", handle, slot.as_u32());
    }

    fn retire_stale_instances(&mut self, scene_manager: &SceneManager) {
        // SecondaryMap 不能在迭代中直接删除，先收集 stale handle，再统一移除并记录 retired slot。
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
                // 延迟到 FIF 窗口之后再复用 slot，保证旧 command buffer 中的 instance index
                // 不会突然指向新实例。
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
        // mesh 必须已经拥有 vertex/index buffer 与 BLAS；material 必须已有稳定 slot。
        // texture 未 ready 不会阻止 material ready，因为 material manager 会使用 fallback binding。
        mesh_resolver.is_mesh_ready(instance.mesh)
            && instance.materials.iter().all(|&material| material_slot_resolver.is_material_ready(material))
    }
}
