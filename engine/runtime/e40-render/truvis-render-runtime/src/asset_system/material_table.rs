use std::collections::HashMap;
use std::mem::size_of;

use ash::vk;
use slotmap::SlotMap;

use truvis_asset::handle::{CoverageMode, MaterialAssetHandle, MaterialClass};
use truvis_asset::material::MaterialData;
use truvis_asset::material_texture::{TextureChannel, TextureTransform};
use truvis_gfx::commands::barrier::{GfxBarrierMask, GfxBufferBarrier};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_shader_binding::gpu;

use crate::bindings::bindless_manager::BindlessSrvHandle;

use super::gpu_texture_store::GpuTextureStore;

const MAX_MATERIAL_COUNT: usize = 1024;

/// 单个 material slot 在 dirty 列表中的 FIF 写入与回收状态。
struct SlotDirtyInfo {
    /// 各 FIF buffer 是否需要更新；true 表示需要写入该帧对应的 GPU buffer。
    fif_dirty: [bool; FrameLabel::COUNT],
    /// 本次 dirty 或 unregister 发生时的 frame id，用于延迟回收计时。
    dirty_frame_id: u64,
}

/// 渲染场景只观察会影响画面或发光采样的投影版本。
#[derive(Clone, Copy, Default)]
pub(crate) struct MaterialRevisions {
    pub(crate) appearance: u64,
    pub(crate) emissive: u64,
}

struct MaterialRecord {
    slot: usize,
    data: MaterialData,
    gpu: gpu::engine::material::PbrMaterial,
    revisions: MaterialRevisions,
    texture_revisions: [bool; TextureChannel::COUNT],
}

/// 单个 FIF frame label 对应的材质 GPU buffer 与 staging buffer。
struct MaterialBuffers {
    /// Device-local SSBO，shader 通过 scene root buffer 中的 device address 读取。
    material_buffer: GfxStructuredBuffer<gpu::engine::material::PbrMaterial>,
    /// Host-mapped staging buffer，CPU 写入后在 prepare 命令中复制到 SSBO。
    material_stage_buffer: GfxStructuredBuffer<gpu::engine::material::PbrMaterial>,
}

impl MaterialBuffers {
    /// 创建单个 FIF frame label 对应的 material device buffer 与 staging buffer。
    ///
    /// material buffer 是 shader 直接读取的 SSBO，stage buffer 用于 prepare 阶段写入
    /// dirty slot 后再 copy 到 device-local buffer。
    fn new(ctx: GfxResourceCtx<'_>, frame_label: FrameLabel) -> Self {
        let material_buffer = GfxStructuredBuffer::new_ssbo(
            ctx,
            MAX_MATERIAL_COUNT,
            format!("MaterialTable::material_buffer-{}", frame_label),
        );
        let stage = {
            GfxStructuredBuffer::new_stage_buffer(
                ctx,
                MAX_MATERIAL_COUNT,
                format!("MaterialTable::material_stage_buffer-{}", frame_label),
            )
        };
        let material_stage_buffer = stage;
        Self {
            material_buffer,
            material_stage_buffer,
        }
    }

    /// 调用方已确认没有在飞使用，依次释放 device 与 staging buffer。
    fn destroy_mut(&mut self, ctx: GfxResourceCtx<'_>, reason: DestroyReason) {
        self.material_buffer.destroy_mut(ctx, reason);
        self.material_stage_buffer.destroy_mut(ctx, reason);
    }
}

/// 共享材质表。注册提交时同时分配 CPU asset handle 与逻辑 GPU slot。
///
/// 各 FIF 副本共享 slot 编号；只写当前安全的副本。删除后旧 slot 跨过整个 FIF 窗口才重用。
pub(super) struct MaterialTable {
    records: SlotMap<MaterialAssetHandle, MaterialRecord>,
    slot_to_handle: Vec<Option<MaterialAssetHandle>>,
    free_slots: Vec<usize>,
    dirty_slots: HashMap<usize, SlotDirtyInfo>,
    buffers: [MaterialBuffers; FrameLabel::COUNT],
    /// 只在 copy/barrier 完整录制后写入，graphics submit 成功后确认对应副本。
    pending_commits: [Vec<usize>; FrameLabel::COUNT],
    current_frame_id: u64,
}

impl MaterialTable {
    pub(super) fn new(ctx: GfxResourceCtx<'_>, current_frame_id: u64) -> Self {
        Self {
            records: SlotMap::with_key(),
            slot_to_handle: vec![None; MAX_MATERIAL_COUNT],
            free_slots: (0..MAX_MATERIAL_COUNT).rev().collect(),
            dirty_slots: HashMap::new(),
            buffers: FrameLabel::ALL.map(|label| MaterialBuffers::new(ctx, label)),
            pending_commits: FrameLabel::ALL.map(|_| Vec::new()),
            current_frame_id,
        }
    }

    pub(super) fn free_slot_count(&self) -> usize {
        self.free_slots.len()
    }

    pub(super) fn register(
        &mut self,
        data: MaterialData,
        gpu_textures: &GpuTextureStore,
    ) -> anyhow::Result<MaterialAssetHandle> {
        data.validate().map_err(anyhow::Error::msg)?;
        let &slot = self.free_slots.last().ok_or_else(|| anyhow::anyhow!("material slots exhausted"))?;
        let gpu = Self::build_gpu_material(&data, gpu_textures);
        let record = MaterialRecord {
            slot,
            texture_revisions: Self::texture_revisions(&data, gpu_textures),
            data,
            gpu,
            revisions: MaterialRevisions {
                appearance: 1,
                emissive: 1,
            },
        };
        self.free_slots.pop();
        let handle = self.records.insert(record);
        self.slot_to_handle[slot] = Some(handle);
        self.mark_dirty(slot);
        Ok(handle)
    }

    pub(super) fn update(
        &mut self,
        handle: MaterialAssetHandle,
        data: MaterialData,
        gpu_textures: &GpuTextureStore,
    ) -> anyhow::Result<bool> {
        data.validate().map_err(anyhow::Error::msg)?;
        let record = self.records.get_mut(handle).ok_or_else(|| anyhow::anyhow!("stale material asset handle"))?;
        if record.data == data {
            return Ok(false);
        }
        if Self::same_render_projection(&record.data, &data) {
            record.data = data;
            return Ok(true);
        }

        let changed_emissive = Self::emissive_projection_changed(&record.data, &data);
        record.gpu = Self::build_gpu_material(&data, gpu_textures);
        record.texture_revisions = Self::texture_revisions(&data, gpu_textures);
        record.data = data;
        record.revisions.appearance = record.revisions.appearance.saturating_add(1);
        if changed_emissive {
            record.revisions.emissive = record.revisions.emissive.saturating_add(1);
        }
        let slot = record.slot;
        self.mark_dirty(slot);
        Ok(true)
    }

    /// 每帧对账已登记材质的纹理状态。可见性变化时重建原 slot 的投影。
    pub(super) fn resolve_textures(&mut self, gpu_textures: &GpuTextureStore) {
        let mut dirty = Vec::new();
        for record in self.records.values_mut() {
            let revisions = Self::texture_revisions(&record.data, gpu_textures);
            if revisions != record.texture_revisions {
                record.texture_revisions = revisions;
                record.gpu = Self::build_gpu_material(&record.data, gpu_textures);
                record.revisions.appearance = record.revisions.appearance.saturating_add(1);
                dirty.push(record.slot);
            }
        }
        for slot in dirty {
            self.mark_dirty(slot);
        }
    }

    fn texture_revisions(data: &MaterialData, gpu_textures: &GpuTextureStore) -> [bool; TextureChannel::COUNT] {
        std::array::from_fn(|index| {
            data.textures[index].as_ref().is_some_and(|slot| gpu_textures.binding(slot.texture).is_some())
        })
    }

    pub(super) fn remove(&mut self, handle: MaterialAssetHandle) -> bool {
        let Some(record) = self.records.remove(handle) else {
            return false;
        };
        let slot = record.slot;
        self.slot_to_handle[slot] = None;
        self.invalidate_recorded_slot(slot);
        self.dirty_slots.insert(
            slot,
            SlotDirtyInfo {
                fif_dirty: [false; FrameLabel::COUNT],
                dirty_frame_id: self.current_frame_id,
            },
        );
        true
    }

    fn mark_dirty(&mut self, slot: usize) {
        self.invalidate_recorded_slot(slot);
        self.dirty_slots.insert(
            slot,
            SlotDirtyInfo {
                fif_dirty: [true; FrameLabel::COUNT],
                dirty_frame_id: self.current_frame_id,
            },
        );
    }

    pub(super) fn destroy(mut self, ctx: GfxResourceCtx<'_>) {
        for buffer in &mut self.buffers {
            buffer.destroy_mut(ctx, DestroyReason::Shutdown);
        }
    }

    pub(super) fn handles(&self) -> impl Iterator<Item = MaterialAssetHandle> + '_ {
        self.records.keys()
    }

    pub(super) fn data(&self, handle: MaterialAssetHandle) -> Option<&MaterialData> {
        self.records.get(handle).map(|record| &record.data)
    }

    pub(super) fn revision(&self, handle: MaterialAssetHandle) -> Option<MaterialRevisions> {
        self.records.get(handle).map(|record| record.revisions)
    }

    pub(super) fn binding(&self, handle: MaterialAssetHandle) -> Option<(u32, &MaterialData)> {
        let record = self.records.get(handle)?;
        Some((record.slot as u32, &record.data))
    }

    pub(super) fn material_buffer_device_address(&self, frame_label: FrameLabel) -> vk::DeviceAddress {
        self.buffers[*frame_label].material_buffer.device_address()
    }
}

// 帧生命周期
impl MaterialTable {
    /// 调用方已等待当前 FIF 安全点；先回收到期空 slot，再允许登记新材质。
    pub fn begin_frame(&mut self, current_frame_id: u64) {
        self.current_frame_id = current_frame_id;
        let retired = self
            .dirty_slots
            .iter()
            .filter_map(|(&slot, info)| {
                (self.slot_to_handle[slot].is_none()
                    && current_frame_id.saturating_sub(info.dirty_frame_id) >= FrameLabel::COUNT as u64)
                    .then_some(slot)
            })
            .collect::<Vec<_>>();
        for slot in retired {
            self.dirty_slots.remove(&slot);
            self.free_slots.push(slot);
            log::debug!("MaterialTable: reclaimed slot={}", slot);
        }
    }

    /// 更新/删除使此前录制的 payload 过期，不能再用旧提交确认清除新 dirty 或退役计时。
    fn invalidate_recorded_slot(&mut self, slot: usize) {
        for pending in &mut self.pending_commits {
            pending.retain(|recorded| *recorded != slot);
        }
    }

    /// 将 dirty slot 写入当前帧对应的 GPU buffer。
    ///
    /// dirty 状态按 FIF buffer 拆分：当前帧只处理 `frame_label` 对应的 staging/device buffer。
    /// 这样每个 frame-in-flight 都能在自己的时机收到材质更新，同时避免覆盖 GPU 仍可能读取的 buffer。
    pub fn upload(
        &mut self,
        ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        frame_label: FrameLabel,
    ) {
        let fif_idx = *frame_label;
        // 每次录制从当前 dirty 重建确认集合，放弃的旧命令不能参与本次提交确认。
        self.pending_commits[fif_idx].clear();

        let mut written_slots: Vec<usize> = Vec::new();

        {
            // stage buffer 的可变借用范围刻意限制在这个 block 内；后续需要再次可变访问
            // dirty_slots 和 buffer owner 来更新状态并提交 copy/barrier。
            let stage_slice = self.buffers[fif_idx].material_stage_buffer.mapped_slice();

            for (&slot, info) in &self.dirty_slots {
                let Some(handle) = self.slot_to_handle[slot] else {
                    continue;
                };

                if !info.fif_dirty[fif_idx] {
                    continue;
                }

                let prepared = &self.records[handle];
                stage_slice[slot] = prepared.gpu;
                written_slots.push(slot);
            }
        }

        if !written_slots.is_empty() {
            let copy_regions = Self::material_copy_regions(&mut written_slots);
            let buf = &mut self.buffers[fif_idx];
            Self::flush_copy_regions_and_barrier(
                ctx,
                cmd,
                &mut buf.material_stage_buffer,
                &mut buf.material_buffer,
                barrier_mask,
                &copy_regions,
            );
            self.pending_commits[fif_idx] = written_slots;
        }
    }

    /// queue submit 成功后确认当前 FIF 的材质写入，避免 command 尚未提交时提前清除 dirty。
    pub(crate) fn commit_submitted_frame(&mut self, frame_label: FrameLabel) {
        let fif_idx = *frame_label;
        let slots = std::mem::take(&mut self.pending_commits[fif_idx]);
        let mut slots_done = Vec::new();
        for slot in slots {
            let Some(info) = self.dirty_slots.get_mut(&slot) else {
                continue;
            };
            info.fif_dirty[fif_idx] = false;
            if info.fif_dirty.iter().all(|&dirty| !dirty) {
                slots_done.push(slot);
            }
        }
        for slot in slots_done {
            self.dirty_slots.remove(&slot);
        }
    }
}

impl MaterialTable {
    fn same_render_projection(left: &MaterialData, right: &MaterialData) -> bool {
        left.base_color == right.base_color
            && left.metallic == right.metallic
            && left.roughness == right.roughness
            && left.class == right.class
            && left.coverage == right.coverage
            && left.textures == right.textures
            && left.normal_scale == right.normal_scale
            && left.emissive_factor == right.emissive_factor
    }

    fn emissive_projection_changed(left: &MaterialData, right: &MaterialData) -> bool {
        left.class != right.class
            || left.emissive_factor != right.emissive_factor
            || left.textures[TextureChannel::Emissive as usize] != right.textures[TextureChannel::Emissive as usize]
    }

    /// 未发布颜色贴图用诊断图，其他通道用各自的中性 null binding。
    fn build_gpu_material(data: &MaterialData, textures: &GpuTextureStore) -> gpu::engine::material::PbrMaterial {
        let textures = std::array::from_fn(|index| {
            let slot = data.textures[index].as_ref();
            let binding = slot.map_or_else(BindlessSrvHandle::null, |slot| {
                textures.binding(slot.texture).unwrap_or_else(|| {
                    if index == TextureChannel::BaseColor as usize {
                        textures.fallback_binding()
                    } else {
                        BindlessSrvHandle::null()
                    }
                })
            });
            let rows = slot.map_or(TextureTransform::default(), |slot| slot.transform).affine_rows();
            gpu::engine::material::MaterialTexture {
                uv_row0: glam::Vec4::from_array(rows[0]).into(),
                uv_row1: glam::Vec4::from_array(rows[1]).into(),
                image: binding.0,
                sampler_type: slot.map_or(gpu::engine::bindless::ESamplerType_LinearRepeat, |slot| {
                    gpu::engine::bindless::ESamplerType_MaterialBase + slot.sampler.index()
                }),
                _padding_0: 0,
                _padding_1: 0,
            }
        });
        gpu::engine::material::PbrMaterial {
            base_color: data.base_color.truncate().into(),
            metallic: data.metallic,
            alpha_factor: data.base_color.w,
            roughness: data.roughness,
            material_class: Self::gpu_material_class(data.class),
            coverage_mode: Self::gpu_coverage_mode(data.coverage),
            opacity: data.class.opacity(),
            ior: data.class.ior(),
            alpha_cutoff: data.coverage.alpha_cutoff(),
            normal_scale: data.normal_scale,
            _padding_0: 0.0,
            emissive: (data.emissive_factor + data.class.emissive_radiance()).into(),
            textures,
        }
    }

    fn gpu_material_class(class: MaterialClass) -> u32 {
        match class {
            MaterialClass::Surface => gpu::engine::material::MATERIAL_CLASS_SURFACE,
            MaterialClass::Transmission { .. } => gpu::engine::material::MATERIAL_CLASS_TRANSMISSION,
            MaterialClass::Emissive { .. } => gpu::engine::material::MATERIAL_CLASS_EMISSIVE,
        }
    }

    fn gpu_coverage_mode(coverage: CoverageMode) -> u32 {
        match coverage {
            CoverageMode::Opaque => gpu::engine::material::COVERAGE_OPAQUE,
            CoverageMode::AlphaMask { .. } => gpu::engine::material::COVERAGE_ALPHA_MASK,
        }
    }

    /// 根据实际写入的 material slot 生成连续 copy regions。
    ///
    /// dirty slot 在 HashMap 中无序保存；上传前按 slot 排序并合并相邻范围，避免把未变化
    /// 的 material 一起复制到 GPU，也避免每个 slot 都录制单独 copy。
    fn material_copy_regions(written_slots: &mut Vec<usize>) -> Vec<vk::BufferCopy> {
        let element_size = size_of::<gpu::engine::material::PbrMaterial>() as vk::DeviceSize;
        debug_assert!(element_size > 0);
        debug_assert_eq!(element_size % 4, 0, "PBRMaterial size must satisfy Vulkan buffer copy alignment");

        written_slots.sort_unstable();
        written_slots.dedup();

        let mut regions = Vec::new();
        let Some(&first_slot) = written_slots.first() else {
            return regions;
        };

        let mut range_start = first_slot;
        let mut prev_slot = first_slot;

        for &slot in written_slots.iter().skip(1) {
            if slot == prev_slot + 1 {
                prev_slot = slot;
                continue;
            }

            regions.push(Self::material_slot_region(range_start, prev_slot, element_size));
            range_start = slot;
            prev_slot = slot;
        }

        regions.push(Self::material_slot_region(range_start, prev_slot, element_size));
        regions
    }

    /// 将闭区间 slot 范围转换为同 offset 的 staging -> device copy region。
    fn material_slot_region(start_slot: usize, end_slot: usize, element_size: vk::DeviceSize) -> vk::BufferCopy {
        let slot_count = end_slot - start_slot + 1;
        let offset = start_slot as vk::DeviceSize * element_size;
        let size = slot_count as vk::DeviceSize * element_size;
        vk::BufferCopy {
            src_offset: offset,
            dst_offset: offset,
            size,
        }
    }

    /// 将当前 staging material buffer 的 dirty regions 刷新、复制到 device buffer，并建立 shader-read barrier。
    ///
    /// `barrier_mask` 来自 `RenderRuntime::prepare_render_world`，和 scene/per-frame buffer 使用同一套可见性约定。
    fn flush_copy_regions_and_barrier(
        ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        stage_buffer: &mut GfxStructuredBuffer<gpu::engine::material::PbrMaterial>,
        dst_buffer: &mut GfxStructuredBuffer<gpu::engine::material::PbrMaterial>,
        barrier_mask: GfxBarrierMask,
        regions: &[vk::BufferCopy],
    ) {
        debug_assert!(!regions.is_empty());

        for region in regions {
            debug_assert!(region.size > 0);
            debug_assert_eq!(region.src_offset, region.dst_offset);
            debug_assert!(region.src_offset + region.size <= stage_buffer.size());
            debug_assert!(region.dst_offset + region.size <= dst_buffer.size());
            stage_buffer.flush(ctx, region.src_offset, region.size);
        }

        cmd.cmd_copy_buffer(stage_buffer, dst_buffer, regions);

        let barriers: Vec<GfxBufferBarrier> = regions
            .iter()
            .map(|region| {
                GfxBufferBarrier::default().mask(barrier_mask).buffer(
                    dst_buffer.vk_buffer(),
                    region.dst_offset,
                    region.size,
                )
            })
            .collect();
        cmd.buffer_memory_barrier(vk::DependencyFlags::empty(), &barriers);
    }
}
