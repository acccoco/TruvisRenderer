use std::collections::HashMap;

use ash::vk;
use slotmap::SlotMap;
use truvis_asset::handle::AssetTextureHandle;
use truvis_gfx::commands::barrier::{GfxBarrierMask, GfxBufferBarrier};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_interface::bindless_manager::BindlessSrvHandle;
use truvis_render_interface::frame_counter::{FrameCounter, FrameToken};
use truvis_render_interface::pipeline_settings::FrameLabel;
use truvis_shader_binding::gpu;

use crate::components::material::ManagedMaterialParams;
use crate::guid_new_type::ManagedMaterialHandle;

const MAX_MATERIAL_COUNT: usize = 1024;

/// Texture 状态查询 trait
///
/// 由外部实现（如 AssetHub + BindlessManager 的组合），在 `update()` / `upload()` 时传入，
/// 避免 MaterialManager 直接耦合 AssetHub。
pub trait TextureResolver {
    /// texture 是否处于 Ready 状态
    fn is_texture_ready(&self, handle: AssetTextureHandle) -> bool;

    /// 获取 texture 在 bindless descriptor 中的 SRV handle。
    /// 返回 None 表示 texture 尚未就绪。
    fn get_srv_handle(&self, handle: AssetTextureHandle) -> Option<BindlessSrvHandle>;
}

/// 单个 slot 中存储的材质元数据
struct MaterialSlotEntry {
    handle: ManagedMaterialHandle,
    params: ManagedMaterialParams,
    /// 各 FIF buffer 是否需要更新（true = 需要写入该帧对应的 GPU buffer）
    fif_dirty: [bool; FrameCounter::fif_count()], // FIXME 这个字段是否合理
    /// 所有引用的 texture 是否全部就绪
    textures_ready: bool, // FIXME 这个字段是否有必要
}

/// 材质 GPU buffer（FIF 套）
struct MaterialBuffers {
    /// Device-local SSBO，shader 直接读取
    material_buffer: GfxStructuredBuffer<gpu::PBRMaterial>,
    /// Host-mapped staging buffer，CPU 写入后 copy 到 SSBO
    material_stage_buffer: GfxStructuredBuffer<gpu::PBRMaterial>,
}

impl MaterialBuffers {
    fn new(frame_label: FrameLabel) -> Self {
        Self {
            material_buffer: GfxStructuredBuffer::new_ssbo(
                MAX_MATERIAL_COUNT,
                format!("MaterialManager::material_buffer-{}", frame_label),
            ),
            material_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                MAX_MATERIAL_COUNT,
                format!("MaterialManager::material_stage_buffer-{}", frame_label),
            ),
        }
    }
}

/// 增量材质管理器
///
/// # 设计
/// 采用 slot + dirty + FIF 延迟回收模式：
/// - 材质注册时分配稳定的 GPU buffer slot，直到删除才释放
/// - 每帧只更新 dirty slot 到当前帧对应的 FIF buffer
/// - 支持 texture 异步依赖：texture 未就绪时使用 INVALID_TEX_ID，就绪后自动标记 dirty
/// - slot 回收延迟 FIF_COUNT 帧，确保 GPU 上不再访问后才归还 free list
///
/// # GPU Buffer 布局
/// 维护 FIF_COUNT 套 `GfxStructuredBuffer<gpu::PBRMaterial>`，每个 slot 对应一个材质。
/// 外部通过 `material_buffer_device_address(frame_label)` 获取 device address，
/// 填入 `GPUScene` 或 push constant 中。
pub struct MaterialManager {
    /// 核心映射：ManagedMaterialHandle -> slot index
    handle_to_slot: SlotMap<ManagedMaterialHandle, usize>,

    /// slot 数据：index = GPU buffer 中的位置
    slots: Vec<Option<MaterialSlotEntry>>,

    /// 空闲 slot 池（降序存放，pop 返回较小 slot）
    free_slots: Vec<usize>, // FIXME 为什么要有序存放

    /// dirty 列表：slot index -> 最后修改时的 frame_id
    dirty_slots: HashMap<usize, u64>,

    /// 待回收列表：slot index -> 删除时的 frame_id
    /// age >= FIF_COUNT 后归还 free_slots
    pending_reclaim: HashMap<usize, u64>,

    /// FIF 套 GPU buffer
    buffers: [MaterialBuffers; FrameCounter::fif_count()],

    frame_token: FrameToken,
}

// new & init
impl MaterialManager {
    pub fn new(frame_token: FrameToken) -> Self {
        let free_slots: Vec<usize> = (0..MAX_MATERIAL_COUNT).rev().collect();
        Self {
            handle_to_slot: SlotMap::with_key(),
            slots: (0..MAX_MATERIAL_COUNT).map(|_| None).collect(),
            free_slots,
            dirty_slots: HashMap::new(),
            pending_reclaim: HashMap::new(),
            buffers: FrameCounter::frame_labes().map(MaterialBuffers::new),
            frame_token,
        }
    }
}

// destroy
impl MaterialManager {
    pub fn destroy(self) {}
}
impl Drop for MaterialManager {
    fn drop(&mut self) {
        log::info!("Dropping MaterialManager");
    }
}

// 注册 / 修改 / 移除
impl MaterialManager {
    /// 注册新材质，分配稳定的 GPU slot
    ///
    /// 返回的 handle 在材质整个生命周期内保持不变，对应的 slot 索引也是稳定的。
    pub fn register(&mut self, params: ManagedMaterialParams) -> ManagedMaterialHandle {
        let slot = self.free_slots.pop().expect("MaterialManager: slots exhausted");
        let handle = self.handle_to_slot.insert(slot);

        self.slots[slot] = Some(MaterialSlotEntry {
            handle,
            params,
            fif_dirty: [true; FrameCounter::fif_count()],
            textures_ready: false,
        });

        self.dirty_slots.insert(slot, self.frame_token.frame_id());
        log::debug!("MaterialManager: register slot={} handle={:?}", slot, handle);
        handle
    }

    /// 更新已注册材质的参数
    ///
    /// 会标记所有 FIF buffer 为 dirty，后续帧会逐个更新。
    pub fn update_params(&mut self, handle: ManagedMaterialHandle, params: ManagedMaterialParams) {
        let &slot = self.handle_to_slot.get(handle).expect("MaterialManager: invalid handle");

        let entry = self.slots[slot].as_mut().expect("MaterialManager: slot is empty");
        entry.params = params;
        entry.fif_dirty = [true; FrameCounter::fif_count()];
        // texture 就绪状态需要重新检测
        entry.textures_ready = false;

        self.dirty_slots.insert(slot, self.frame_token.frame_id());
    }

    /// 移除材质
    ///
    /// slot 不会立即回收，而是进入 pending_reclaim 等待 FIF_COUNT 帧后才归还，
    /// 确保 GPU 上所有 in-flight 命令不再访问该 slot。
    pub fn unregister(&mut self, handle: ManagedMaterialHandle) {
        let slot = self.handle_to_slot.remove(handle).expect("MaterialManager: invalid handle");

        self.slots[slot] = None;
        self.dirty_slots.remove(&slot);
        self.pending_reclaim.insert(slot, self.frame_token.frame_id());

        log::debug!("MaterialManager: unregister slot={} handle={:?}", slot, handle);
    }
}

// 帧生命周期
impl MaterialManager {
    /// 帧开始时调用，更新 frame_token 并回收过期 slot
    pub fn begin_frame(&mut self, frame_token: FrameToken) {
        self.frame_token = frame_token;

        let fif = FrameCounter::fif_count() as u64;
        let frame_id = frame_token.frame_id();

        let mut reclaimed = Vec::new();
        for (&slot, &remove_frame_id) in &self.pending_reclaim {
            let age = frame_id.saturating_sub(remove_frame_id);
            if age >= fif {
                reclaimed.push(slot);
            }
        }
        for slot in reclaimed {
            self.pending_reclaim.remove(&slot);
            self.free_slots.push(slot);
            log::debug!("MaterialManager: reclaimed slot={}", slot);
        }
    }

    /// 检查 texture 异步加载状态，就绪时标记 dirty
    ///
    /// 在 `upload()` 之前调用。对每个尚未标记 `textures_ready` 的材质，
    /// 通过 `TextureResolver` 查询所有引用的 texture 是否就绪。
    /// 如果全部就绪且此前未就绪，则重新标记 `fif_dirty = [true; FIF]`。
    pub fn update(&mut self, texture_resolver: &dyn TextureResolver) {
        for slot_entry in self.slots.iter_mut().flatten() {
            if slot_entry.textures_ready {
                continue;
            }

            let all_ready = Self::check_textures_ready(&slot_entry.params, texture_resolver);
            if all_ready {
                slot_entry.textures_ready = true;
                // texture 刚变为就绪，需要重新上传到所有 FIF buffer（之前用的是 placeholder）
                slot_entry.fif_dirty = [true; FrameCounter::fif_count()];
                self.dirty_slots.insert(self.handle_to_slot[slot_entry.handle], self.frame_token.frame_id());
            }
        }
    }

    /// 将 dirty slot 写入当前帧对应的 GPU buffer
    ///
    /// 只更新 `fif_dirty[frame_label] == true` 的 slot。
    /// texture 未就绪的材质使用 `INVALID_TEX_ID` 作为 placeholder。
    pub fn upload(
        &mut self,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        frame_label: FrameLabel,
        texture_resolver: &dyn TextureResolver,
    ) {
        let fif_idx = *frame_label;
        let buf = &mut self.buffers[fif_idx];
        let stage_slice = buf.material_stage_buffer.mapped_slice();
        let mut any_written = false;

        // 收集需要处理的 dirty slot
        let dirty_slot_indices: Vec<usize> = self.dirty_slots.keys().copied().collect();

        let mut slots_done: Vec<usize> = Vec::new();
        for slot in dirty_slot_indices {
            let entry = match &self.slots[slot] {
                Some(e) => e,
                None => continue,
            };

            if !entry.fif_dirty[fif_idx] {
                continue;
            }

            stage_slice[slot] = Self::build_gpu_material(&entry.params, texture_resolver);
            any_written = true;

            // 标记当前帧已更新
            let entry_mut = self.slots[slot].as_mut().unwrap();
            entry_mut.fif_dirty[fif_idx] = false;

            // 如果所有帧都已更新，从 dirty_slots 中移除
            if entry_mut.fif_dirty.iter().all(|&d| !d) {
                slots_done.push(slot);
            }
        }

        for slot in slots_done {
            self.dirty_slots.remove(&slot);
        }

        if any_written {
            Self::flush_copy_and_barrier(cmd, &mut buf.material_stage_buffer, &mut buf.material_buffer, barrier_mask);
        }
    }
}

// getter
impl MaterialManager {
    /// 获取材质在 GPU buffer 中的 slot index
    #[inline]
    pub fn get_slot_index(&self, handle: ManagedMaterialHandle) -> Option<usize> {
        self.handle_to_slot.get(handle).copied()
    }

    /// 获取指定帧的 material buffer device address
    #[inline]
    pub fn material_buffer_device_address(&self, frame_label: FrameLabel) -> vk::DeviceAddress {
        self.buffers[*frame_label].material_buffer.device_address()
    }

    /// 获取材质的 CPU 参数
    #[inline]
    pub fn get_params(&self, handle: ManagedMaterialHandle) -> Option<&ManagedMaterialParams> {
        let &slot = self.handle_to_slot.get(handle)?;
        self.slots[slot].as_ref().map(|e| &e.params)
    }

    /// 获取当前已注册的材质数量
    #[inline]
    pub fn material_count(&self) -> usize {
        self.handle_to_slot.len()
    }

    /// 获取当前 dirty 的 slot 数量
    #[inline]
    pub fn dirty_count(&self) -> usize {
        self.dirty_slots.len()
    }
}

// 内部工具方法
impl MaterialManager {
    fn check_textures_ready(params: &ManagedMaterialParams, resolver: &dyn TextureResolver) -> bool {
        if let Some(h) = params.diffuse_texture {
            if !resolver.is_texture_ready(h) {
                return false;
            }
        }
        if let Some(h) = params.normal_texture {
            if !resolver.is_texture_ready(h) {
                return false;
            }
        }
        true
    }

    // TODO 是否可以改成 Default texture，而不是 null
    fn build_gpu_material(params: &ManagedMaterialParams, resolver: &dyn TextureResolver) -> gpu::PBRMaterial {
        let diffuse_srv =
            params.diffuse_texture.and_then(|h| resolver.get_srv_handle(h)).unwrap_or(BindlessSrvHandle::null());
        let normal_srv =
            params.normal_texture.and_then(|h| resolver.get_srv_handle(h)).unwrap_or(BindlessSrvHandle::null());

        gpu::PBRMaterial {
            base_color: params.base_color.truncate().into(),
            emissive: params.emissive.truncate().into(),
            metallic: params.metallic,
            roughness: params.roughness,
            diffuse_map: diffuse_srv.0,
            diffuse_map_sampler_type: gpu::ESamplerType_LinearRepeat,
            normal_map: normal_srv.0,
            normal_map_sampler_type: gpu::ESamplerType_LinearRepeat,
            opaque: params.opaque,
            _padding_1: Default::default(),
            _padding_2: Default::default(),
            _padding_3: Default::default(),
        }
    }

    // TODO 可以细化更新 regions
    fn flush_copy_and_barrier(
        cmd: &GfxCommandBuffer,
        stage_buffer: &mut GfxStructuredBuffer<gpu::PBRMaterial>,
        dst_buffer: &mut GfxStructuredBuffer<gpu::PBRMaterial>,
        barrier_mask: GfxBarrierMask,
    ) {
        let buffer_size = stage_buffer.size();
        stage_buffer.flush(0, buffer_size);
        cmd.cmd_copy_buffer(
            stage_buffer,
            dst_buffer,
            &[vk::BufferCopy {
                size: buffer_size,
                ..Default::default()
            }],
        );
        cmd.buffer_memory_barrier(
            vk::DependencyFlags::empty(),
            &[GfxBufferBarrier::default().mask(barrier_mask).buffer(dst_buffer.vk_buffer(), 0, vk::WHOLE_SIZE)],
        );
    }
}
