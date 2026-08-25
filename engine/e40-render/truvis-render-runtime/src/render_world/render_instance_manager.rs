use std::collections::{HashMap, HashSet};

use slotmap::SecondaryMap;

use truvis_render_foundation::frame_counter::{FrameLabel, FrameToken};
use truvis_world::SceneReadView;
use truvis_world::components::instance::Instance;
use truvis_world::guid_new_type::{InstanceHandle, MaterialHandle, MeshHandle};

use crate::render_world::dirty_router::InstanceDispatch;
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

/// instance 阶段对 dirty routing 暴露的结构化结果。
#[derive(Default)]
pub(crate) struct RenderInstanceUpdateResult {
    pub(crate) active_set_changed: bool,
    pub(crate) transform_changed: bool,
    pub(crate) material_binding_changed: bool,
    pub(crate) mesh_binding_changed: bool,
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

    /// 将 CPU `InstanceHandle + submesh_index` 解析为当前 prepare 快照里的稳定 GPU draw key。
    ///
    /// 该接口只做只读查询，不激活 pending instance，也不重新遍历 CPU scene。`ray_cast_records`
    /// 与 raster draw cache 在同一次 prepare 中生成，因此这里用它校验 slot 仍属于同一个
    /// `InstanceHandle`，并用 material 数量作为 instance-local submesh 边界。
    pub(crate) fn resolve_active_raster_submesh(
        &self,
        instance: InstanceHandle,
        submesh_index: u32,
    ) -> Option<(u32, u32)> {
        let binding = self.bindings.get(instance)?;
        if binding.state != InstanceState::Active {
            return None;
        }

        let slot = binding.slot.as_u32();
        let record = self.ray_cast_record(slot)?;
        if record.instance != instance || submesh_index as usize >= record.materials.len() {
            return None;
        }

        Some((slot, submesh_index))
    }

    /// 从 CPU scene read view 构建本帧可渲染的 `RenderData` 快照。
    ///
    /// 该阶段会先同步 instance 生命周期与依赖 ready 状态；只有 mesh/material 都能被 resolver
    /// 解析到 GPU 数据的实例才进入 active 列表。输出按稳定 slot 排序，保证 raster draw、
    /// TLAS custom index 和 GPU instance buffer 共享同一套 instance slot 语义。
    pub fn prepare_render_data<'a>(
        &mut self,
        scene: SceneReadView<'_>,
        dispatch: InstanceDispatch,
        material_slot_resolver: &dyn MaterialSlotResolver,
        mesh_resolver: &'a dyn MeshRenderResolver,
    ) -> (RenderData<'a>, RenderInstanceUpdateResult) {
        let update_result = self.sync_scene_instances(scene, dispatch, material_slot_resolver, mesh_resolver);

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
            let mesh_geometry_count = all_meshes[mesh_index].geometries.len();
            if mesh_geometry_count != instance.materials.len() {
                log::error!(
                    "RenderInstanceManager: skip instance {:?}; material count {} does not match mesh {:?} geometry count {}",
                    handle,
                    instance.materials.len(),
                    instance.mesh,
                    mesh_geometry_count
                );
                continue;
            }

            let mut material_slots = Vec::with_capacity(instance.materials.len());
            let mut requires_any_hit = false;
            for &material in &instance.materials {
                let Some(slot) = material_slot_resolver.resolve_material_slot(material) else {
                    continue 'active;
                };
                let Some(data) = scene.material_data(material) else {
                    continue 'active;
                };
                requires_any_hit |= data.coverage.requires_any_hit();
                material_slots.push(slot);
            }

            all_instances.push(InstanceRenderData {
                instance_slot: binding.slot,
                mesh_index,
                material_slots,
                material_handles: instance.materials.clone(),
                requires_any_hit,
                transform: instance.transform,
                previous_transform: binding.previous_transform,
            });
            self.ray_cast_records[binding.slot.as_usize()] = Some(RayCastInstanceRecord {
                instance: handle,
                mesh: instance.mesh,
                materials: instance.materials.clone(),
            });
        }

        let render_data = RenderData {
            all_instances,
            all_meshes,
            mesh_geometry_start_indices,
        };
        (render_data, update_result)
    }

    fn sync_scene_instances(
        &mut self,
        scene: SceneReadView<'_>,
        dispatch: InstanceDispatch,
        material_slot_resolver: &dyn MaterialSlotResolver,
        mesh_resolver: &dyn MeshRenderResolver,
    ) -> RenderInstanceUpdateResult {
        let mut result = RenderInstanceUpdateResult::default();
        let reset_motion_history = self.motion_history_reset_pending;
        let InstanceDispatch {
            dirty_instances,
            removed_instances,
        } = dispatch;

        // 先处理 router 显式分发的删除，避免后续 active 列表继续输出 stale slot。
        for handle in removed_instances {
            if self.retire_instance_binding(handle) {
                result.active_set_changed = true;
            }
        }

        for (&handle, _) in &dirty_instances {
            let Some(instance) = scene.get_instance(handle) else {
                continue;
            };
            if !self.bindings.contains_key(handle) {
                self.register_instance(handle, instance);
            }
        }

        // stale 扫描是 manager 自身状态的防御性清理，不作为 dirty 传播来源；
        // 正常路径仍应由 `SceneInstanceRemoved` rule 显式分发删除。
        let stale_handles = self
            .bindings
            .iter()
            .filter_map(|(handle, _)| scene.get_instance(handle).is_none().then_some(handle))
            .collect::<Vec<_>>();
        for handle in stale_handles {
            if self.retire_instance_binding(handle) {
                result.active_set_changed = true;
            }
        }

        let dirty_handles = dirty_instances.keys().copied().collect::<HashSet<_>>();
        for (handle, binding) in self.bindings.iter_mut() {
            let Some(instance) = scene.get_instance(handle) else {
                continue;
            };
            let flags = dirty_instances.get(&handle).copied().unwrap_or_default();

            if reset_motion_history {
                // 历史重置只影响 previous transform，不表示 CPU scene 语义变化；这里不推进
                // scene revision，除非 CPU transform 在同一帧确实发生变化。
                let transform_changed = binding.last_transform != instance.transform;
                binding.previous_transform = instance.transform;
                binding.last_transform = instance.transform;
                if transform_changed && binding.state == InstanceState::Active {
                    self.revision = self.revision.saturating_add(1);
                    result.transform_changed = true;
                }
            } else if flags.transform {
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
                    result.transform_changed = true;
                }
            } else {
                // motion vector 需要逐帧推进 previous_transform。这个维护不是 dirty 传播，
                // 而是 temporal history 的帧生命周期不变量。
                binding.previous_transform = binding.last_transform;
            }

            if flags.material_binding && binding.state == InstanceState::Active {
                // material list 或 material class/coverage 变化会改变 instance material indirect map、emissive table，
                // 也可能改变 TLAS FORCE_OPAQUE 派生规则；dirty router 会据此推进 TLAS。
                self.revision = self.revision.saturating_add(1);
                result.material_binding_changed = true;
            }
            if flags.mesh_binding && binding.state == InstanceState::Active {
                // mesh ready 或替换会改变 instance -> BLAS/geometry 的关系，需要让 TLAS 和
                // emissive table 重新读取 active instance 快照。
                self.revision = self.revision.saturating_add(1);
                result.mesh_binding_changed = true;
            }

            if !flags.needs_ready_check() && !dirty_handles.contains(&handle) {
                continue;
            }

            // ready gate 由 material/mesh resolver 共同决定。instance manager 不直接访问 material/mesh manager
            // 内部缓存，只依赖窄接口判断这个实例是否可以进入本帧 render data。
            let ready = Self::dependencies_ready(instance, material_slot_resolver, mesh_resolver);
            match (binding.state, ready) {
                (InstanceState::Pending, true) => {
                    // mesh/material 都 ready 后才激活，避免 draw/TLAS 使用空 BLAS 或无效 material slot。
                    binding.state = InstanceState::Active;
                    binding.previous_transform = instance.transform;
                    self.revision = self.revision.saturating_add(1);
                    result.active_set_changed = true;
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
                    result.active_set_changed = true;
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
        result
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

    fn retire_instance_binding(&mut self, handle: InstanceHandle) -> bool {
        if let Some(binding) = self.bindings.remove(handle) {
            let was_active = binding.state == InstanceState::Active;
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
            return was_active;
        }
        false
    }

    fn reclaim_retired_slots(&mut self) {
        let current_frame_id = self.frame_token.frame_id();
        let fif_count = FrameLabel::COUNT as u64;
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
        let Some(mesh) = mesh_resolver.resolve_mesh(instance.mesh) else {
            return false;
        };
        mesh.geometries.len() == instance.materials.len()
            && instance.materials.iter().all(|&material| material_slot_resolver.is_material_ready(material))
    }
}
