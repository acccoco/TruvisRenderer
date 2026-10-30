use std::collections::HashMap;
use std::mem::{offset_of, size_of};

use ash::vk;
use indexmap::IndexSet;
use slotmap::SecondaryMap;

use truvis_render_foundation::frame_label::FrameLabel;
use truvis_shader_binding::gpu;
use truvis_world::SceneReadView;
use truvis_world::components::instance::Instance;
use truvis_world::guid_new_type::{MaterialAssetHandle, MeshAssetHandle, MeshInstanceHandle};

use crate::render_world::render_data::{GpuInstanceSlot, InstanceRenderData, MeshRenderData, RenderData};
use crate::render_world::render_resolver::{MaterialSlotResolver, MeshRenderResolver};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstanceState {
    /// 已分配稳定 slot，但 mesh/material 依赖尚未全部 GPU-ready。
    Pending,
    /// 可进入 `RenderData`，对应 slot 会被写入 instance buffer 和 TLAS custom index。
    Active,
}

/// 单个 runtime instance 在 render-side 的稳定绑定。
///
/// `last_submitted_transform` 是运动历史的唯一提交基准；prepare 期间不会提前推进它。
struct InstanceBinding {
    slot: GpuInstanceSlot,
    state: InstanceState,
    /// 最终对账后保存的渲染相关 instance 副本；打包阶段不回读 CPU GameWorld。
    source: Instance,
    source_revision_seen: u64,
    requires_any_hit: bool,
    current_version: u64,
    last_submitted_transform: glam::Mat4,
    /// None 表示本身份尚未参与主视图提交，首次上传令 previous=current。
    last_submitted_version: Option<u64>,
    /// 只记录已入队的 GPU 写入；不能把该 FIF 上次使用的 current 当作全局 previous。
    uploaded_versions: [Option<(u64, u64)>; FrameLabel::COUNT],
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
    pub(crate) instance: MeshInstanceHandle,
    pub(crate) mesh: MeshAssetHandle,
    pub(crate) materials: Vec<MaterialAssetHandle>,
}

/// Render-side runtime instance manager.
///
/// 它为 `MeshInstanceHandle` 分配生命周期内稳定的 GPU instance slot，并在 mesh/material
/// 都 GPU ready 前保持 pending，避免 draw/TLAS 访问未就绪资源。
/// manager 是 CPU scene read view 与 runtime 私有 `RenderData` 之间的翻译层：`GameWorld`
/// 保存语义实例，RenderWorld 只接收按稳定 slot 排序、依赖已就绪的渲染快照。
pub struct RenderInstanceTable {
    bindings: SecondaryMap<MeshInstanceHandle, InstanceBinding>,
    /// 镜像首次建立时读取完整 membership，后续只处理编辑/资源发布候选。
    initialized: bool,
    /// 已接管的最终状态候选；整批对账成功后清空，异常时保留以便重新读取 CPU 最终状态。
    pending_candidates: IndexSet<MeshInstanceHandle>,
    /// 状态已经改变但尚未成功打包交付的通知，不能因下一次对账成为 no-op 而丢失。
    pending_update: RenderInstanceUpdateResult,
    free_slots: Vec<GpuInstanceSlot>,
    retired_slots: Vec<RetiredSlot>,
    current_frame_id: u64,
    ray_cast_records: Vec<Option<RayCastInstanceRecord>>,
    dirty_instances: IndexSet<MeshInstanceHandle>,
    /// 只有 copy/barrier 完整录制后才接管；未确认时不能开始下一次 prepare。
    pending_upload: Option<PreparedInstanceUpload>,
    pending_history: Vec<(MeshInstanceHandle, u64)>,
}

/// 本次矩阵写入的准备结果；CPU 打包成功不等于 command 已录制或 queue 已提交。
/// regions 由调用者录制，成功后整份结果移交回 table，避免失败时提前确认副本或运动历史。
pub(crate) struct PreparedInstanceUpload {
    pub(crate) regions: Vec<vk::BufferCopy>,
    frame_label: FrameLabel,
    versions: Vec<(MeshInstanceHandle, (u64, u64))>,
    history: Vec<(MeshInstanceHandle, u64)>,
}

/// instance 阶段对 RenderWorld 对账暴露的结构化结果。
#[derive(Default)]
pub(crate) struct RenderInstanceUpdateResult {
    pub(crate) active_set_changed: bool,
    pub(crate) transform_changed: bool,
    pub(crate) requires_any_hit_changed: bool,
    pub(crate) material_binding_changed: bool,
}

impl RenderInstanceTable {
    /// 创建 instance manager；slot 随场景增长，删除后的 slot 仍跨过 FIF 窗口才复用。
    pub fn new(current_frame_id: u64) -> Self {
        Self {
            bindings: SecondaryMap::new(),
            initialized: false,
            pending_candidates: IndexSet::new(),
            pending_update: RenderInstanceUpdateResult::default(),
            free_slots: Vec::new(),
            retired_slots: Vec::new(),
            current_frame_id,
            ray_cast_records: Vec::new(),
            dirty_instances: IndexSet::new(),
            pending_upload: None,
            pending_history: Vec::new(),
        }
    }

    /// 帧开始时推进 frame id，并回收已经跨过 FIF 窗口的 retired slot。
    pub fn begin_frame(&mut self, current_frame_id: u64) {
        // slot 回收以 frame id 为准推进；每帧开始时回收已经跨过 FIF 窗口的旧 slot。
        self.current_frame_id = current_frame_id;
        self.reclaim_retired_slots();
    }

    /// 调用方已完整录制本次 copy/barrier；此后提交失败时保留记录，不能隐式清空重试。
    pub(crate) fn record_transform_uploads(&mut self, upload: PreparedInstanceUpload) {
        assert!(self.pending_upload.is_none() && self.pending_history.is_empty(), "previous frame was not finished");
        for &(handle, _) in &upload.versions {
            self.dirty_instances.insert(handle);
        }
        self.pending_upload = Some(upload);
    }

    /// 上传 submit 正常返回后确认当前 FIF 的写入；不推进主视图运动历史。
    pub(crate) fn commit_uploaded_frame(&mut self, frame_label: FrameLabel) {
        let upload = self.pending_upload.as_ref().expect("instance upload was not recorded");
        assert_eq!(*upload.frame_label, *frame_label, "instance upload confirmed for another FIF");
        for &(handle, (current, previous)) in &upload.versions {
            let binding = self.bindings.get(handle).expect("uploaded instance disappeared within frame");
            assert_eq!(binding.current_version, current, "instance changed after recording");
            assert_eq!(binding.last_submitted_version.unwrap_or(current), previous, "instance history changed after recording");
        }
        for &(handle, version) in &upload.history {
            let binding = self.bindings.get(handle).expect("recorded instance history disappeared within frame");
            assert_eq!(binding.current_version, version, "instance changed after recording");
        }
        for &(handle, versions) in &upload.versions {
            self.bindings.get_mut(handle).unwrap().uploaded_versions[*frame_label] = Some(versions);
        }
        self.pending_history = self.pending_upload.take().unwrap().history;
    }

    /// Loop 在主视图 render 返回后调用；source 从 prepare 到此处保持冻结。
    /// 未提交主视图时仅释放帧内记录，下帧仍从上一实际提交的 transform 回溯。
    pub(crate) fn finish_rendered_frame(&mut self, scene_submitted: bool) {
        assert!(self.pending_upload.is_none(), "instance upload was not confirmed before render");
        if scene_submitted {
            // 全部身份与版本先通过检查，再推进任何实例历史，避免后项失效留下部分确认。
            for &(handle, version) in &self.pending_history {
                let binding = self.bindings.get(handle).expect("rendered instance disappeared within frame");
                assert_eq!(binding.current_version, version, "instance changed after prepare");
            }
            for &handle in &self.dirty_instances {
                assert!(self.bindings.contains_key(handle), "dirty instance disappeared within frame");
            }
            for &(handle, version) in &self.pending_history {
                let binding = self.bindings.get_mut(handle).unwrap();
                binding.last_submitted_transform = binding.source.transform;
                binding.last_submitted_version = Some(version);
            }
            self.dirty_instances.retain(|handle| {
                let binding = &self.bindings[*handle];
                let version = binding.current_version;
                binding.last_submitted_version != Some(version)
                    || binding.uploaded_versions.iter().any(|pair| *pair != Some((version, version)))
            });
        }
        self.pending_history.clear();
    }

    /// 结构上传强制重写所有 Active record；确认版本与 CPU 运动历史在此阶段均不改变。
    /// 部分 stage 内容在失败后不可提交，调用者必须放弃对应 command；准备结果不隐式提交。
    pub(crate) fn prepare_transform_uploads(
        &self,
        frame_label: FrameLabel,
        full_records: bool,
        stage: &mut [gpu::engine::scene::Instance],
    ) -> PreparedInstanceUpload {
        assert!(self.pending_upload.is_none() && self.pending_history.is_empty(), "previous frame was not finished");
        let handles = if full_records {
            self.bindings.iter().filter(|(_, binding)| binding.state == InstanceState::Active)
                .map(|(handle, _)| handle).collect::<Vec<_>>()
        } else {
            self.dirty_instances.iter().copied().collect::<Vec<_>>()
        };
        let mut upload = PreparedInstanceUpload {
            regions: Vec::new(), frame_label, versions: Vec::new(), history: Vec::new(),
        };
        for handle in handles {
            let binding = &self.bindings[handle];
            let current = binding.current_version;
            let previous = binding.last_submitted_version.unwrap_or(current);
            let uploaded = binding.uploaded_versions[*frame_label];
            let write_current = full_records || uploaded.is_none_or(|pair| pair.0 != current);
            let write_previous = full_records || uploaded.is_none_or(|pair| pair.1 != previous);
            let slot = binding.slot.as_usize();
            let record = &mut stage[slot];
            if write_current {
                record.model = binding.source.transform.into();
                record.inv_model = binding.source.transform.inverse().into();
            }
            if write_previous {
                record.prev_model = if binding.last_submitted_version.is_some() {
                    binding.last_submitted_transform
                } else {
                    binding.source.transform
                }
                .into();
            }
            if full_records {
                upload.regions.push(Self::instance_region(slot, 0, size_of::<gpu::engine::scene::Instance>()));
            } else {
                if write_current {
                    upload.regions.push(Self::instance_region(
                        slot,
                        offset_of!(gpu::engine::scene::Instance, model),
                        size_of_val(&record.model),
                    ));
                    upload.regions.push(Self::instance_region(
                        slot,
                        offset_of!(gpu::engine::scene::Instance, inv_model),
                        size_of_val(&record.inv_model),
                    ));
                }
                if write_previous {
                    upload.regions.push(Self::instance_region(
                        slot,
                        offset_of!(gpu::engine::scene::Instance, prev_model),
                        size_of_val(&record.prev_model),
                    ));
                }
            }
            if write_current || write_previous {
                upload.versions.push((handle, (current, previous)));
            }
            if binding.last_submitted_version != Some(current) {
                upload.history.push((handle, current));
            }
        }
        if !upload.regions.is_empty() {
            log::debug!(
                "Instance upload: FIF={:?}, dirty={}, regions={}, bytes={}, full={}",
                frame_label,
                self.dirty_instances.len(),
                upload.regions.len(),
                upload.regions.iter().map(|r| r.size).sum::<u64>(),
                full_records
            );
        }
        upload
    }

    fn instance_region(slot: usize, field_offset: usize, size: usize) -> vk::BufferCopy {
        let offset = (slot * size_of::<gpu::engine::scene::Instance>() + field_offset) as vk::DeviceSize;
        vk::BufferCopy {
            src_offset: offset,
            dst_offset: offset,
            size: size as vk::DeviceSize,
        }
    }

    /// 读取当前 prepare 快照中的 raycast 反查记录。
    pub(crate) fn ray_cast_record(&self, instance_slot: u32) -> Option<&RayCastInstanceRecord> {
        self.ray_cast_records.get(instance_slot as usize)?.as_ref()
    }

    /// 将 CPU `MeshInstanceHandle + submesh_index` 解析为当前 prepare 快照里的稳定 GPU draw key。
    ///
    /// 该接口只做只读查询，不激活 pending instance，也不重新遍历 CPU scene。`ray_cast_records`
    /// 与 raster draw cache 在同一次 prepare 中生成，因此这里用它校验 slot 仍属于同一个
    /// `MeshInstanceHandle`，并用 material 数量作为 instance-local submesh 边界。
    pub(crate) fn resolve_active_raster_submesh(
        &self,
        instance: MeshInstanceHandle,
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

    /// 在任何可能失败的 prepare 工作之前接管身份；对账继续读取 CPU 最终状态。
    pub(crate) fn queue_changes(&mut self, candidates: IndexSet<MeshInstanceHandle>) {
        self.pending_candidates.extend(candidates);
    }

    /// 从 CPU scene read view 构建本帧可渲染的 `RenderData` 快照。
    ///
    /// 该阶段会先同步 instance 生命周期与依赖 ready 状态；只有 mesh/material 都能被 resolver
    /// 解析到 GPU 数据的实例才进入 active 列表。输出按稳定 slot 排序，保证 raster draw、
    /// TLAS custom index 和 GPU instance buffer 共享同一套 instance slot 语义。
    pub fn prepare_render_data<'a>(
        &mut self,
        scene: SceneReadView<'_>,
        material_slot_resolver: &dyn MaterialSlotResolver,
        mesh_resolver: &'a dyn MeshRenderResolver,
    ) -> (RenderData<'a>, RenderInstanceUpdateResult) {
        assert!(self.pending_upload.is_none() && self.pending_history.is_empty(), "previous frame was not finished");
        if !self.initialized {
            self.pending_candidates.extend(scene.instance_map().keys());
            self.initialized = true;
        }
        self.sync_scene_instances(scene, material_slot_resolver, mesh_resolver);

        // RenderData 是提交给 RenderWorld 的只读快照。这里按稳定 slot 排序，保证 raster draw、
        // TLAS custom index 和 GPU instance buffer 使用同一套 instance slot 语义。
        let mut mesh_handle_to_index = HashMap::new();
        let mut all_meshes: Vec<MeshRenderData<'a>> = Vec::new();
        let mut mesh_geometry_start_indices: Vec<usize> = Vec::new();
        let mut total_geometry_count = 0usize;
        let mut all_instances: Vec<InstanceRenderData> = Vec::new();
        self.ray_cast_records.fill(None);

        let mut active_instances = self
            .bindings
            .iter()
            .filter(|(_, binding)| binding.state == InstanceState::Active)
            .map(|(handle, binding)| (handle, binding, &binding.source))
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
                total_geometry_count = total_geometry_count
                    .checked_add(mesh_render_data.geometries.len())
                    .expect("render geometry count overflow");
                all_meshes.push(mesh_render_data);
                index
            };
            let mesh_geometry_count = all_meshes[mesh_index].geometries.len();
            if mesh_geometry_count != instance.materials.len() {
                log::error!(
                    "RenderInstanceTable: skip instance {:?}; material count {} does not match mesh {:?} geometry count {}",
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
                let Some(data) = material_slot_resolver.material_data(material) else {
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
        (render_data, std::mem::take(&mut self.pending_update))
    }

    fn sync_scene_instances(
        &mut self,
        scene: SceneReadView<'_>,
        material_slot_resolver: &dyn MaterialSlotResolver,
        mesh_resolver: &dyn MeshRenderResolver,
    ) {
        if !self.pending_candidates.is_empty() {
            log::debug!("RenderInstanceTable: reconcile {} candidate instances", self.pending_candidates.len());
        }
        // 整批完成前保留候选及顺序；失败重入对账是幂等的，已产生的通知单独保留。
        for index in 0..self.pending_candidates.len() {
            let handle = self.pending_candidates[index];
            let Some(instance) = scene.get_instance(handle) else {
                self.pending_update.active_set_changed |= self.retire_instance_binding(handle);
                continue;
            };
            let source_revision =
                scene.instance_revision(handle).expect("RenderInstanceTable: live instance revision missing");
            // resolver 与版本预检先完成，不能先推进 source revision 再丢失派生变化。
            let requires_any_hit = instance.materials.iter().any(|&material| {
                material_slot_resolver.material_data(material).is_some_and(|data| data.coverage.requires_any_hit())
            });
            let ready = Self::dependencies_ready(instance, material_slot_resolver, mesh_resolver);
            if !self.bindings.contains_key(handle) {
                self.register_instance(handle, instance, source_revision);
            }
            let binding = self.bindings.get_mut(handle).expect("instance binding just installed");
            let source_changed = binding.source_revision_seen != source_revision;
            let transform_changed = source_changed && binding.source.transform != instance.transform;
            let material_binding_changed = source_changed && binding.source.materials != instance.materials;
            let current_version = if transform_changed {
                binding.current_version.checked_add(1).expect("instance transform version overflow")
            } else {
                binding.current_version
            };
            let source = source_changed.then(|| instance.clone());
            let was_active = binding.state == InstanceState::Active;
            if transform_changed && was_active {
                log::debug!(
                    "RenderInstanceTable: transform dirty handle={:?} stable_slot={}", handle, binding.slot.as_u32()
                );
            }
            if was_active != ready {
                log::trace!(
                    "RenderInstanceTable: {} handle={:?} stable_slot={}",
                    if ready { "activate" } else { "deactivate" }, handle, binding.slot.as_u32()
                );
            }

            // 本身份的查询、诊断与可失败预检均成功后，再一起提交镜像及下游变更标记。
            if let Some(source) = source {
                binding.source = source;
                binding.source_revision_seen = source_revision;
            }
            binding.current_version = current_version;
            if transform_changed && was_active {
                self.dirty_instances.insert(handle);
                self.pending_update.transform_changed = true;
            }
            self.pending_update.material_binding_changed |= material_binding_changed && was_active;
            self.pending_update.requires_any_hit_changed |= was_active && binding.requires_any_hit != requires_any_hit;
            binding.requires_any_hit = requires_any_hit;
            match (binding.state, ready) {
                (InstanceState::Pending, true) => {
                    binding.state = InstanceState::Active;
                    binding.last_submitted_version = None;
                    binding.uploaded_versions.fill(None);
                    self.dirty_instances.insert(handle);
                    self.pending_update.active_set_changed = true;
                }
                (InstanceState::Active, false) => {
                    binding.state = InstanceState::Pending;
                    binding.last_submitted_version = None;
                    self.dirty_instances.swap_remove(&handle);
                    self.pending_update.active_set_changed = true;
                }
                _ => {}
            }
        }
        self.pending_candidates.clear();
    }

    fn register_instance(&mut self, handle: MeshInstanceHandle, instance: &Instance, source_revision: u64) {
        // 新实例先拿到稳定 slot，但初始状态保持 pending；ready gate 由 resolver 决定。
        let slot = self.free_slots.last().copied().unwrap_or_else(|| {
            let index = u32::try_from(self.ray_cast_records.len())
                .expect("RenderInstanceTable: GPU instance slot index exceeds u32");
            let slot = GpuInstanceSlot::new(index);
            slot.validate_tlas_custom_index();
            slot
        });
        let source = instance.clone();
        log::trace!("RenderInstanceTable: register handle={:?} stable_slot={}", handle, slot.as_u32());
        if self.free_slots.pop().is_none() {
            self.ray_cast_records.push(None);
        }
        self.bindings.insert(
            handle,
            InstanceBinding {
                slot,
                state: InstanceState::Pending,
                source,
                source_revision_seen: source_revision,
                requires_any_hit: false,
                current_version: 1,
                last_submitted_transform: instance.transform,
                last_submitted_version: None,
                uploaded_versions: [None; FrameLabel::COUNT],
            },
        );
    }

    fn retire_instance_binding(&mut self, handle: MeshInstanceHandle) -> bool {
        if let Some(binding) = self.bindings.get(handle) {
            log::debug!(
                "RenderInstanceTable: retire handle={:?} stable_slot={}; reclaim delayed by FIF",
                handle, binding.slot.as_u32()
            );
            let binding = self.bindings.remove(handle).expect("retired instance binding disappeared");
            self.dirty_instances.swap_remove(&handle);
            let was_active = binding.state == InstanceState::Active;
            self.retired_slots.push(RetiredSlot {
                slot: binding.slot,
                retired_frame_id: self.current_frame_id,
            });
            return was_active;
        }
        false
    }

    fn reclaim_retired_slots(&mut self) {
        let current_frame_id = self.current_frame_id;
        let fif_count = FrameLabel::COUNT as u64;
        // 记录到期 slot 后转入 free list；尚未跨过 FIF 窗口的 slot 继续保留。
        for retired in &self.retired_slots {
            if current_frame_id.saturating_sub(retired.retired_frame_id) >= fif_count {
                log::debug!("RenderInstanceTable: reclaimed stable_slot={}", retired.slot.as_u32());
            }
        }
        self.retired_slots.retain(|retired| {
            if current_frame_id.saturating_sub(retired.retired_frame_id) >= fif_count {
                self.free_slots.push(retired.slot);
                false
            } else {
                true
            }
        });
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
