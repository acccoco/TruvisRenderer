use std::collections::{HashMap, HashSet};

use slotmap::SecondaryMap;

use truvis_render_foundation::frame_counter::{FrameCounter, FrameToken};
use truvis_shader_binding::gpu;
use truvis_world::components::instance::Instance;
use truvis_world::guid_new_type::{InstanceHandle, MaterialHandle, MeshHandle};
use truvis_world::{SceneChanges, SceneInstanceChangeKind, SceneReadView};

use crate::render_world::render_data::{GpuInstanceSlot, InstanceRenderData, MeshRenderData, RenderData};
use crate::render_world::render_resolver::{MaterialSlotResolver, MeshRenderResolver};

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
    previous_transform: glam::Mat4,
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
/// 保证 after_prepare 中的同步查询能把 GPU world 结果还原成 CPU world handle。
#[derive(Clone)]
pub(crate) struct RayCastInstanceRecord {
    pub(crate) instance: InstanceHandle,
    pub(crate) mesh: MeshHandle,
    pub(crate) materials: Vec<MaterialHandle>,
}

/// Render-side runtime instance manager.
///
/// 它为 `InstanceHandle` 分配生命周期内稳定的 GPU instance slot，并在 mesh/material
/// 都 GPU ready 前保持 pending，避免 draw/TLAS 访问未就绪资源。
/// manager 是 CPU scene read view 与 runtime 私有 `RenderData` 之间的翻译层：`World`
/// 保存语义实例，RenderWorld 只接收按稳定 slot 排序、依赖已就绪的渲染快照。
pub struct RenderInstanceManager {
    bindings: SecondaryMap<InstanceHandle, InstanceBinding>,
    free_slots: Vec<GpuInstanceSlot>,
    retired_slots: Vec<RetiredSlot>,
    frame_token: FrameToken,
    revision: u64,
    ray_cast_records: Vec<Option<RayCastInstanceRecord>>,
    motion_history_reset_pending: bool,
}

impl RenderInstanceManager {
    /// 创建 instance manager，并预分配稳定 GPU instance slot 池。
    ///
    /// slot 数量当前与 `RenderWorld` instance buffer 容量保持一致；耗尽表示 CPU scene 中可渲染实例
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
            motion_history_reset_pending: true,
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

    /// 请求下一次 prepare 把所有 active instance 的 motion history 对齐到当前 transform。
    ///
    /// DLSS history reset 后，上一帧输出已不可复用；即使 CPU transform 没变，也不能继续把
    /// 旧模型矩阵写给 motion vector shader，否则第一帧会产生不对应任何 DLSS history 的向量。
    pub fn request_motion_history_reset(&mut self) {
        self.motion_history_reset_pending = true;
    }

    /// 读取当前 prepare 快照中的 raycast 反查记录。
    pub(crate) fn ray_cast_record(&self, instance_slot: u32) -> Option<&RayCastInstanceRecord> {
        self.ray_cast_records.get(instance_slot as usize)?.as_ref()
    }

    /// 从 CPU scene read view 构建本帧可渲染的 `RenderData` 快照。
    ///
    /// 该阶段会先同步 instance 生命周期与依赖 ready 状态；只有 mesh/material 都能被 resolver
    /// 解析到 GPU 数据的实例才进入 active 列表。输出按稳定 slot 排序，保证 raster draw、
    /// TLAS custom index 和 GPU instance buffer 共享同一套 instance slot 语义。
    pub fn prepare_render_data<'a>(
        &mut self,
        scene: SceneReadView<'_>,
        scene_changes: &SceneChanges,
        material_slot_resolver: &dyn MaterialSlotResolver,
        mesh_resolver: &'a dyn MeshRenderResolver,
    ) -> RenderData<'a> {
        self.sync_scene_instances(scene, scene_changes, material_slot_resolver, mesh_resolver);

        // RenderData 是提交给 RenderWorld 的只读快照。这里按稳定 slot 排序，保证 raster draw、
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
                    .then(|| scene.get_instance(handle).map(|instance| (handle, binding, instance)))
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
                material_handles: instance.materials.clone(),
                transform: instance.transform,
                previous_transform: binding.previous_transform,
            });
            self.ray_cast_records[binding.slot.as_usize()] = Some(RayCastInstanceRecord {
                instance: handle,
                mesh: instance.mesh,
                materials: instance.materials.clone(),
            });
        }

        let all_point_lights: Vec<gpu::light::PointLight> =
            scene.point_light_map().iter().map(|(_, light)| *light).collect();
        let all_spot_lights: Vec<gpu::light::SpotLight> =
            scene.spot_light_map().iter().map(|(_, light)| *light).collect();
        let all_area_lights: Vec<gpu::light::AreaLight> =
            scene.area_light_map().iter().map(|(_, light)| *light).collect();
        // analytic_light_version 跟随 CPU scene 语义快照一起传递到 RenderData。
        // RenderInstanceManager 不解释 ReSTIR，也不修改 light 采样概率，只保持 prepare 边界的数据一致性。
        let analytic_light_version = scene.light_revision();

        RenderData {
            all_instances,
            all_meshes,
            all_point_lights,
            all_spot_lights,
            all_area_lights,
            analytic_light_version,
            mesh_geometry_start_indices,
        }
    }

    fn sync_scene_instances(
        &mut self,
        scene: SceneReadView<'_>,
        scene_changes: &SceneChanges,
        material_slot_resolver: &dyn MaterialSlotResolver,
        mesh_resolver: &dyn MeshRenderResolver,
    ) {
        let reset_motion_history = self.motion_history_reset_pending;
        // 先处理 CPU scene 显式删除的实例，避免后续 active 列表继续输出 stale slot。
        for &handle in &scene_changes.removed_instances {
            self.retire_instance_binding(handle);
        }

        let changed_instances = scene_changes
            .changed_instances
            .iter()
            .map(|change| (change.handle, change.kind))
            .collect::<HashMap<_, _>>();
        let transform_changed_instances = changed_instances
            .iter()
            .filter_map(|(&handle, &kind)| {
                matches!(kind, SceneInstanceChangeKind::Lifecycle | SceneInstanceChangeKind::Transform)
                    .then_some(handle)
            })
            .collect::<HashSet<_>>();

        for change in &scene_changes.changed_instances {
            let Some(instance) = scene.get_instance(change.handle) else {
                continue;
            };
            if !self.bindings.contains_key(change.handle) {
                self.register_instance(change.handle, instance);
            }
        }

        let stale_handles = self
            .bindings
            .iter()
            .filter_map(|(handle, _)| scene.get_instance(handle).is_none().then_some(handle))
            .collect::<Vec<_>>();
        for handle in stale_handles {
            self.retire_instance_binding(handle);
        }

        for (handle, binding) in self.bindings.iter_mut() {
            let Some(instance) = scene.get_instance(handle) else {
                continue;
            };
            // ready gate 由 material/mesh resolver 共同决定。instance manager 不直接访问 material/mesh manager
            // 内部缓存，只依赖窄接口判断这个实例是否可以进入本帧 render data。
            let ready = Self::dependencies_ready(instance, material_slot_resolver, mesh_resolver);

            if reset_motion_history {
                // 历史重置只影响 previous transform，不表示 CPU scene 语义变化；这里不推进
                // scene revision，除非 CPU transform 在同一帧确实发生变化。
                let transform_changed = binding.last_transform != instance.transform;
                binding.previous_transform = instance.transform;
                binding.last_transform = instance.transform;
                if transform_changed && binding.state == InstanceState::Active {
                    self.revision = self.revision.saturating_add(1);
                }
            } else if transform_changed_instances.contains(&handle) || binding.last_transform != instance.transform {
                binding.previous_transform = binding.last_transform;
                binding.last_transform = instance.transform;
                if binding.state == InstanceState::Active {
                    // transform 变化会影响 instance buffer 与 TLAS transform，
                    // revision 用来让 RenderWorld 知道当前帧需要重建 TLAS。
                    self.revision = self.revision.saturating_add(1);
                    log::debug!(
                        "RenderInstanceManager: transform dirty handle={:?} stable_slot={}",
                        handle,
                        binding.slot.as_u32()
                    );
                }
            } else {
                binding.previous_transform = binding.last_transform;
            }

            if matches!(changed_instances.get(&handle), Some(SceneInstanceChangeKind::MaterialBinding))
                && binding.state == InstanceState::Active
            {
                // material list 变化不会影响 TLAS，但会改变 instance material indirect map 和
                // emissive table。当前 scene buffer 每帧上传，revision 用于驱动依赖该语义的派生表。
                self.revision = self.revision.saturating_add(1);
            }

            match (binding.state, ready) {
                (InstanceState::Pending, true) => {
                    // mesh/material 都 ready 后才激活，避免 draw/TLAS 使用空 BLAS 或无效 material slot。
                    binding.state = InstanceState::Active;
                    binding.previous_transform = instance.transform;
                    self.revision = self.revision.saturating_add(1);
                    log::trace!(
                        "RenderInstanceManager: activate handle={:?} stable_slot={}",
                        handle,
                        binding.slot.as_u32()
                    );
                }
                (InstanceState::Active, false) => {
                    // asset 重新加载或材质被移除时，已激活实例会退回 pending，
                    // 直到 resolver 再次提供完整 GPU 数据。
                    binding.state = InstanceState::Pending;
                    self.revision = self.revision.saturating_add(1);
                    log::trace!(
                        "RenderInstanceManager: deactivate handle={:?} stable_slot={}",
                        handle,
                        binding.slot.as_u32()
                    );
                }
                _ => {}
            }
        }

        self.motion_history_reset_pending = false;
    }

    fn register_instance(&mut self, handle: InstanceHandle, instance: &Instance) {
        // 新实例先拿到稳定 slot，但初始状态保持 pending；ready gate 由 resolver 决定。
        let slot = self.free_slots.pop().expect("RenderInstanceManager: GPU instance slots exhausted");
        self.bindings.insert(
            handle,
            InstanceBinding {
                slot,
                state: InstanceState::Pending,
                last_transform: instance.transform,
                previous_transform: instance.transform,
            },
        );
        log::trace!("RenderInstanceManager: register handle={:?} stable_slot={}", handle, slot.as_u32());
    }

    fn retire_instance_binding(&mut self, handle: InstanceHandle) {
        if let Some(binding) = self.bindings.remove(handle) {
            if binding.state == InstanceState::Active {
                self.revision = self.revision.saturating_add(1);
            }
            self.retired_slots.push(RetiredSlot {
                slot: binding.slot,
                retired_frame_id: self.frame_token.frame_id(),
            });
            log::debug!(
                "RenderInstanceManager: retire handle={:?} stable_slot={}; reclaim delayed by FIF",
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
                log::debug!("RenderInstanceManager: reclaimed stable_slot={}", retired.slot.as_u32());
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
