use std::{collections::{HashMap, HashSet}, mem::size_of};

use ash::vk;
use slotmap::SecondaryMap;

use truvis_gfx::commands::barrier::{GfxBarrierMask, GfxBufferBarrier};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_shader_binding::gpu;
use truvis_world::SceneReadView;
use truvis_world::components::material::{CoverageMode, MaterialClass, MaterialData};
use truvis_world::guid_new_type::MaterialHandle;

use crate::render_world::render_resolver::MaterialSlotResolver;
use crate::render_world::texture_resolver::{TextureBinding, TextureResolver};

const MAX_MATERIAL_COUNT: usize = 1024;

/// 单个 material slot 在 dirty 列表中的 FIF 写入与回收状态。
struct SlotDirtyInfo {
    /// 各 FIF buffer 是否需要更新；true 表示需要写入该帧对应的 GPU buffer。
    fif_dirty: [bool; FrameLabel::COUNT],
    /// 本次 dirty 或 unregister 发生时的 frame id，用于延迟回收计时。
    dirty_frame_id: u64,
}

/// 同一次资源对账发布的材质语义与 GPU payload。
struct PreparedMaterial {
    data: MaterialData,
    gpu: gpu::engine::material::PbrMaterial,
    diffuse_texture_revision: u64,
    normal_texture_revision: u64,
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
        Self {
            material_buffer: GfxStructuredBuffer::new_ssbo(
                ctx,
                MAX_MATERIAL_COUNT,
                format!("RenderMaterialManager::material_buffer-{}", frame_label),
            ),
            material_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                ctx,
                MAX_MATERIAL_COUNT,
                format!("RenderMaterialManager::material_stage_buffer-{}", frame_label),
            ),
        }
    }

    /// 销毁该 FIF 的 material buffer 对。
    fn destroy_mut(&mut self, ctx: GfxResourceCtx<'_>) {
        self.material_buffer.destroy_mut(ctx, DestroyReason::Shutdown);
        self.material_stage_buffer.destroy_mut(ctx, DestroyReason::Shutdown);
    }
}

/// 增量材质管理器
///
/// 将 GPU material slot 映射、dirty 状态和增量上传逻辑聚合为独立模块，
/// 而非分散在 SceneStore（CPU 数据 owner）和 RenderWorld（阶段编排）之间。
/// 这与 `BindlessManager` 的设计模式一致——每种 GPU 资源由专门的 Manager 自治管理。
/// 在 runtime 分层中，它是 scene material 进入 shader 可见 buffer 的唯一 owner。
///
/// # Slot 稳定性
///
/// 每个注册的材质对应一个固定的 GPU buffer slot，直到删除才释放。
///
/// # Dirty 和延迟回收
///
/// - 每帧只更新 dirty slot 到当前帧对应的 FIF buffer
/// - slot 延迟回收：当 slot 内容删除且 frame 间隔 >= FIF_COUNT 时才归还 free list，
///   确保所有引用该 slot 的 in-flight 命令已完成。
///
/// # 无阻塞异步 Texture
///
/// 材质注册后即可被外部安全引用，无论其 texture 是否就绪。
/// texture 异步加载过程中使用占位数据（null texture），就绪后自动标记 dirty 并更新到 GPU。
/// GPU 端始终有合法数据可用。
pub struct RenderMaterialManager {
    /// 核心映射：MaterialHandle -> shader 可见 material buffer slot。
    ///
    /// render-side bridge 直接以 CPU `MaterialHandle` 作为 key，不再额外引入第二套 GPU material handle。
    handle_to_slot: SecondaryMap<MaterialHandle, usize>,

    /// 每个 slot 最近一次观察到的 CPU material revision。
    source_revisions: SecondaryMap<MaterialHandle, u64>,

    /// 对账后持久保存的渲染副本；GPU 上传与场景派生不再回读 CPU ResourceSystem。
    prepared_materials: SecondaryMap<MaterialHandle, PreparedMaterial>,

    /// slot 数据：index = GPU buffer 中的位置；None 表示已 unregister、等待延迟回收。
    ///
    /// dirty upload 通过 handle 读取上面已发布的 prepared snapshot。
    slot_to_handle: Vec<Option<MaterialHandle>>,

    /// 可立即分配的 slot。被删除的 slot 必须跨过 FIF 窗口后才能回到这里。
    free_slots: Vec<usize>,

    /// dirty 列表：slot index -> SlotDirtyInfo，记录每个 FIF buffer 是否还需要补写该 slot。
    dirty_slots: HashMap<usize, SlotDirtyInfo>,

    /// FIF 套 GPU buffer，避免 CPU 覆盖 GPU 仍在读取的 material buffer。
    buffers: [MaterialBuffers; FrameLabel::COUNT],

    /// 已录入当前 command buffer、等待 queue submit 确认的 slot。
    pending_commits: [Vec<usize>; FrameLabel::COUNT],

    current_frame_id: u64,
}

// 创建与初始化
impl RenderMaterialManager {
    /// 创建 FIF 套材质 buffer，并初始化可分配 slot 池。
    pub fn new(ctx: GfxResourceCtx<'_>, current_frame_id: u64) -> Self {
        let free_slots: Vec<usize> = (0..MAX_MATERIAL_COUNT).rev().collect();
        Self {
            handle_to_slot: SecondaryMap::new(),
            source_revisions: SecondaryMap::new(),
            prepared_materials: SecondaryMap::new(),
            slot_to_handle: (0..MAX_MATERIAL_COUNT).map(|_| None).collect(),
            free_slots,
            dirty_slots: HashMap::new(),
            buffers: FrameLabel::ALL.map(|frame_label| MaterialBuffers::new(ctx, frame_label)),
            pending_commits: FrameLabel::ALL.map(|_| Vec::new()),
            current_frame_id,
        }
    }
}

/// material 阶段对资源对账暴露的结构化结果。
#[derive(Default)]
pub(crate) struct RenderMaterialUpdateResult {
    /// 本帧渲染投影发生变化的材质；RenderWorld 只对实际被 active instance 使用的材质失效历史。
    pub(crate) appearance_changed_materials: Vec<MaterialHandle>,
    /// 会改变 emissive table 输入的材质；普通 roughness/normal texture 变化不进入此列表。
    pub(crate) emissive_changed_materials: Vec<MaterialHandle>,
}

// 销毁
impl RenderMaterialManager {
    /// 从完整 CPU material registry 对账 stable slot 和待上传状态。
    ///
    /// 该扫描替代 material dirty event 的可靠消费要求。source revision 只决定是否重新读取
    /// CPU 状态；重新读取后还要比较渲染投影和每个实际纹理依赖，名称等 metadata 变化不会
    /// 制造 GPU 上传或场景历史失效。
    pub(crate) fn sync_scene(
        &mut self,
        scene: SceneReadView<'_>,
        texture_resolver: &dyn TextureResolver,
    ) -> RenderMaterialUpdateResult {
        let mut result = RenderMaterialUpdateResult::default();
        let live_handles = scene.material_handles().collect::<HashSet<_>>();
        let stale_handles = self
            .handle_to_slot
            .keys()
            .filter(|handle| !live_handles.contains(handle))
            .collect::<Vec<_>>();

        for handle in stale_handles {
            self.unregister(handle);
        }

        for handle in scene.material_handles() {
            let source_revision = scene
                .material_revision(handle)
                .expect("RenderMaterialManager: material handle disappeared during scene scan");
            let data = scene.material_data(handle).expect("RenderMaterialManager: missing source material");
            let dependency_revisions = Self::texture_dependency_revisions(data, texture_resolver);
            if !self.handle_to_slot.contains_key(handle) {
                let data = data.clone();
                let gpu = Self::build_gpu_material(&data, texture_resolver);
                self.prepared_materials.insert(
                    handle,
                    PreparedMaterial {
                        data,
                        gpu,
                        diffuse_texture_revision: dependency_revisions.0,
                        normal_texture_revision: dependency_revisions.1,
                    },
                );
                self.register(handle);
                self.source_revisions.insert(handle, source_revision);
                result.appearance_changed_materials.push(handle);
                result.emissive_changed_materials.push(handle);
                continue;
            }

            let source_changed = self.source_revisions.get(handle).copied() != Some(source_revision);
            let Some(previous) = self.prepared_materials.get(handle) else {
                panic!("RenderMaterialManager: material slot has no prepared snapshot");
            };
            let dependency_changed = previous.diffuse_texture_revision != dependency_revisions.0
                || previous.normal_texture_revision != dependency_revisions.1;
            if source_changed || dependency_changed {
                let data = data.clone();
                let gpu = Self::build_gpu_material(&data, texture_resolver);
                let appearance_changed = !Self::same_render_projection(&previous.data, &data)
                    || dependency_changed;
                let emissive_changed = Self::emissive_projection_changed(&previous.data, &data);
                self.prepared_materials.insert(
                    handle,
                    PreparedMaterial {
                        data,
                        gpu,
                        diffuse_texture_revision: dependency_revisions.0,
                        normal_texture_revision: dependency_revisions.1,
                    },
                );
                self.source_revisions.insert(handle, source_revision);
                if appearance_changed {
                    self.update_material(handle);
                    result.appearance_changed_materials.push(handle);
                }
                if emissive_changed {
                    result.emissive_changed_materials.push(handle);
                }
            }
        }

        result
    }

    /// 销毁所有 FIF material buffer。
    pub fn destroy(mut self, ctx: GfxResourceCtx<'_>) {
        for buffer in &mut self.buffers {
            buffer.destroy_mut(ctx);
        }
    }
}
impl Drop for RenderMaterialManager {
    fn drop(&mut self) {
        log::info!("Dropping RenderMaterialManager");
    }
}

// 注册 / 修改 / 移除
impl RenderMaterialManager {
    /// 注册新材质，分配稳定的 GPU slot。
    ///
    /// `MaterialHandle` 是 CPU material identity；GPU 侧只额外维护稳定 slot，
    /// 不再引入第二套长期 material handle。
    fn register(&mut self, handle: MaterialHandle) {
        let slot = self.free_slots.pop().expect("RenderMaterialManager: slots exhausted");

        self.handle_to_slot.insert(handle, slot);
        self.slot_to_handle[slot] = Some(handle);
        self.dirty_slots.insert(
            slot,
            SlotDirtyInfo {
                fif_dirty: [true; FrameLabel::COUNT],
                dirty_frame_id: self.current_frame_id,
            },
        );
        log::trace!("RenderMaterialManager: register scene_handle={:?} stable_slot={}", handle, slot);
    }

    /// 更新已注册材质的 dirty 状态。
    ///
    /// 会标记所有 FIF buffer 为 dirty，后续帧逐个上传当前 prepared snapshot。
    fn update_material(&mut self, handle: MaterialHandle) {
        let &slot = self.handle_to_slot.get(handle).expect("RenderMaterialManager: invalid handle");

        self.slot_to_handle[slot] = Some(handle);

        let frame_id = self.current_frame_id;
        self.dirty_slots
            .entry(slot)
            .and_modify(|info| {
                info.fif_dirty = [true; FrameLabel::COUNT];
                info.dirty_frame_id = frame_id;
            })
            .or_insert(SlotDirtyInfo {
                fif_dirty: [true; FrameLabel::COUNT],
                dirty_frame_id: frame_id,
            });

        log::debug!(
            "RenderMaterialManager: update scene_handle={:?} stable_slot={}; dirty all FIF buffers",
            handle,
            slot
        );
    }

    /// 移除材质，延迟回收 slot
    ///
    /// slot 内容不再上传，但 slot index 会继续保留至少 `FIF_COUNT` 帧，避免在飞命令仍用旧 index
    /// 访问 material buffer 时被新材质复用。
    fn unregister(&mut self, handle: MaterialHandle) -> bool {
        let Some(slot) = self.handle_to_slot.remove(handle) else {
            log::debug!("RenderMaterialManager: ignore unregister for unknown handle={:?}", handle);
            return false;
        };

        self.source_revisions.remove(handle);
        self.prepared_materials.remove(handle);

        self.slot_to_handle[slot] = None;
        // fif_dirty 全设为 false：不再需要上传，仅保留 dirty_frame_id 用于回收计时
        let frame_id = self.current_frame_id;
        self.dirty_slots
            .entry(slot)
            .and_modify(|info| {
                info.fif_dirty = [false; FrameLabel::COUNT];
                info.dirty_frame_id = frame_id;
            })
            .or_insert(SlotDirtyInfo {
                fif_dirty: [false; FrameLabel::COUNT],
                dirty_frame_id: frame_id,
            });

        log::debug!("RenderMaterialManager: unregister slot={} handle={:?}", slot, handle);
        true
    }
}

// 帧生命周期
impl RenderMaterialManager {
    /// 帧开始时调用，更新后续 dirty/回收判断使用的 frame id。
    pub fn begin_frame(&mut self, current_frame_id: u64) {
        // 实际回收发生在 upload 中，因为回收判断需要和当前 FIF dirty 状态处理保持同一处。
        self.current_frame_id = current_frame_id;
    }

    /// 将 dirty slot 写入当前帧对应的 GPU buffer，或者回收 slot 到 free list 中
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
        let fif_count = FrameLabel::COUNT as u64;
        let current_frame_id = self.current_frame_id;

        let dirty_slot_indices: Vec<usize> = self.dirty_slots.keys().copied().collect();

        let mut written_slots: Vec<usize> = Vec::new();
        let mut slots_to_reclaim: Vec<usize> = Vec::new();

        {
            // stage buffer 的可变借用范围刻意限制在这个 block 内；后续需要再次可变访问
            // dirty_slots 和 buffer owner 来更新状态并提交 copy/barrier。
            let stage_slice = self.buffers[fif_idx].material_stage_buffer.mapped_slice();

            for &slot in &dirty_slot_indices {
                let info = &self.dirty_slots[&slot];

                let Some(handle) = self.slot_to_handle[slot] else {
                    // slot 已删除：检查回收计时
                    let age = current_frame_id.saturating_sub(info.dirty_frame_id);
                    if age >= fif_count {
                        slots_to_reclaim.push(slot);
                    }
                    continue;
                };

                if !info.fif_dirty[fif_idx] {
                    continue;
                }

                let prepared = self.prepared_materials.get(handle).expect("RenderMaterialManager: missing prepared material");
                stage_slice[slot] = prepared.gpu;
                written_slots.push(slot);
            }
        }

        for slot in slots_to_reclaim {
            self.dirty_slots.remove(&slot);
            self.free_slots.push(slot);
            log::debug!("RenderMaterialManager: reclaimed slot={}", slot);
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
            self.pending_commits[fif_idx].extend(written_slots);
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

// 访问器
impl RenderMaterialManager {
    /// 获取材质在 GPU buffer 中的 slot index
    #[inline]
    pub fn get_slot_index(&self, handle: MaterialHandle) -> Option<usize> {
        self.handle_to_slot.get(handle).copied()
    }

    /// 获取指定帧的 material buffer device address
    #[inline]
    pub fn material_buffer_device_address(&self, frame_label: FrameLabel) -> vk::DeviceAddress {
        self.buffers[*frame_label].material_buffer.device_address()
    }
}

impl MaterialSlotResolver for RenderMaterialManager {
    fn resolve_material_slot(&self, handle: MaterialHandle) -> Option<u32> {
        // resolver 是 RenderInstanceManager 能看到的唯一 material 接口；找不到 binding 表示
        // CPU scene 仍引用了未加载或已删除的 material，实例应保持 pending。
        let slot = self.get_slot_index(handle)?;
        u32::try_from(slot).ok()
    }

    fn material_data(&self, handle: MaterialHandle) -> Option<&MaterialData> {
        self.prepared_materials.get(handle).map(|prepared| &prepared.data)
    }
}

// 内部工具方法
impl RenderMaterialManager {
    fn texture_dependency_revisions(data: &MaterialData, resolver: &dyn TextureResolver) -> (u64, u64) {
        (
            data.diffuse_texture.map_or(0, |handle| resolver.texture_revision(handle)),
            data.normal_texture.map_or(0, |handle| resolver.texture_revision(handle)),
        )
    }

    fn same_render_projection(left: &MaterialData, right: &MaterialData) -> bool {
        left.base_color == right.base_color
            && left.metallic == right.metallic
            && left.roughness == right.roughness
            && left.class == right.class
            && left.coverage == right.coverage
            && left.diffuse_texture == right.diffuse_texture
            && left.normal_texture == right.normal_texture
    }

    fn emissive_projection_changed(left: &MaterialData, right: &MaterialData) -> bool {
        left.class != right.class
            || left.base_color != right.base_color
            || left.diffuse_texture.is_some() != right.diffuse_texture.is_some()
    }

    /// 将 CPU 材质参数转换为 shader 读取的 packed GPU 数据。
    ///
    /// texture handle 在这里通过 resolver 转成 bindless SRV index；resolver 保证未 ready
    /// 的 texture 也会返回 fallback，因此 GPU 数据不会包含悬空句柄。
    fn build_gpu_material(data: &MaterialData, resolver: &dyn TextureResolver) -> gpu::engine::material::PbrMaterial {
        let diffuse_binding =
            data.diffuse_texture.map(|h| resolver.resolve_texture(h)).unwrap_or(TextureBinding::null());
        let normal_binding = data.normal_texture.map(|h| resolver.resolve_texture(h)).unwrap_or(TextureBinding::null());

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
            _padding_0: 0.0,
            emissive: data.class.emissive_radiance().into(),
            _padding_1: 0.0,
            diffuse_map: diffuse_binding.srv_handle.0,
            diffuse_map_sampler_type: diffuse_binding.sampler,
            normal_map: normal_binding.srv_handle.0,
            normal_map_sampler_type: normal_binding.sampler,
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
