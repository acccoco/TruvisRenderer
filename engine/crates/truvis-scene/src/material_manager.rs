use std::collections::{HashMap, HashSet};

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

/// 单个 slot 在 dirty_slots 中维护的状态
struct SlotDirtyInfo {
    /// 各 FIF buffer 是否需要更新（true = 需要写入该帧对应的 GPU buffer）
    fif_dirty: [bool; FrameCounter::fif_count()],
    /// 本次 dirty（或 unregister）发生时的 frame_id，用于回收计时
    dirty_frame_id: u64,
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
/// - slot 延迟回收：当 slot 内容删除且 frame 间隔 >= FIF_COUNT 时才归还 free list
pub struct MaterialManager {
    /// 核心映射：ManagedMaterialHandle -> slot index
    handle_to_slot: SlotMap<ManagedMaterialHandle, usize>,

    /// slot 数据：index = GPU buffer 中的位置
    slots: Vec<Option<ManagedMaterialParams>>,

    free_slots: Vec<usize>,

    /// dirty 列表：slot index -> SlotDirtyInfo
    dirty_slots: HashMap<usize, SlotDirtyInfo>,

    /// 等待 texture 就绪的材质 handle 列表
    pending_texture_ready: HashSet<ManagedMaterialHandle>,

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
            pending_texture_ready: HashSet::new(),
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

        let has_textures = params.diffuse_texture.is_some() || params.normal_texture.is_some();
        self.slots[slot] = Some(params);
        self.dirty_slots.insert(
            slot,
            SlotDirtyInfo {
                fif_dirty: [true; FrameCounter::fif_count()],
                dirty_frame_id: self.frame_token.frame_id(),
            },
        );
        if has_textures {
            self.pending_texture_ready.insert(handle);
        }

        log::debug!("MaterialManager: register slot={} handle={:?}", slot, handle);
        handle
    }

    /// 更新已注册材质的参数
    ///
    /// 会标记所有 FIF buffer 为 dirty，后续帧会逐个更新。
    pub fn update_params(&mut self, handle: ManagedMaterialHandle, params: ManagedMaterialParams) {
        let &slot = self.handle_to_slot.get(handle).expect("MaterialManager: invalid handle");

        let has_textures = params.diffuse_texture.is_some() || params.normal_texture.is_some();
        self.slots[slot] = Some(params);

        let frame_id = self.frame_token.frame_id();
        self.dirty_slots
            .entry(slot)
            .and_modify(|info| {
                info.fif_dirty = [true; FrameCounter::fif_count()];
                info.dirty_frame_id = frame_id;
            })
            .or_insert(SlotDirtyInfo {
                fif_dirty: [true; FrameCounter::fif_count()],
                dirty_frame_id: frame_id,
            });

        // texture 就绪状态需要重新检测
        if has_textures {
            self.pending_texture_ready.insert(handle);
        } else {
            self.pending_texture_ready.remove(&handle);
        }
    }

    /// 移除材质，延迟回收 slot
    pub fn unregister(&mut self, handle: ManagedMaterialHandle) {
        let slot = self.handle_to_slot.remove(handle).expect("MaterialManager: invalid handle");

        self.slots[slot] = None;
        self.pending_texture_ready.remove(&handle);
        // fif_dirty 全设为 false：不再需要上传，仅保留 dirty_frame_id 用于回收计时
        let frame_id = self.frame_token.frame_id();
        self.dirty_slots
            .entry(slot)
            .and_modify(|info| {
                info.fif_dirty = [false; FrameCounter::fif_count()];
                info.dirty_frame_id = frame_id;
            })
            .or_insert(SlotDirtyInfo {
                fif_dirty: [false; FrameCounter::fif_count()],
                dirty_frame_id: frame_id,
            });

        log::debug!("MaterialManager: unregister slot={} handle={:?}", slot, handle);
    }
}

// 帧生命周期
impl MaterialManager {
    /// 帧开始时调用，更新 frame_token 并回收过期 slot
    pub fn begin_frame(&mut self, frame_token: FrameToken) {
        self.frame_token = frame_token;
    }

    /// 检查 texture 异步加载状态，尝试新增 dirty 标记
    pub fn update(&mut self, texture_resolver: &dyn TextureResolver) {
        let frame_id = self.frame_token.frame_id();

        let now_ready: Vec<ManagedMaterialHandle> = self
            .pending_texture_ready
            .iter()
            .copied()
            .filter(|&handle| {
                let slot = self.handle_to_slot[handle];
                let entry = self.slots[slot].as_ref().unwrap();
                Self::check_textures_ready(entry, texture_resolver)
            })
            .collect();

        for handle in now_ready {
            self.pending_texture_ready.remove(&handle);
            let slot = self.handle_to_slot[handle];
            // texture 刚变为就绪，需要重新上传到所有 FIF buffer（之前用的是 placeholder）
            self.dirty_slots
                .entry(slot)
                .and_modify(|info| {
                    info.fif_dirty = [true; FrameCounter::fif_count()];
                    info.dirty_frame_id = frame_id;
                })
                .or_insert(SlotDirtyInfo {
                    fif_dirty: [true; FrameCounter::fif_count()],
                    dirty_frame_id: frame_id,
                });
        }
    }

    /// 将 dirty slot 写入当前帧对应的 GPU buffer，或者回收 slot 到 free list 中
    pub fn upload(
        &mut self,
        cmd: &GfxCommandBuffer,
        barrier_mask: GfxBarrierMask,
        frame_label: FrameLabel,
        texture_resolver: &dyn TextureResolver,
    ) {
        let fif_idx = *frame_label;
        let fif_count = FrameCounter::fif_count() as u64;
        let current_frame_id = self.frame_token.frame_id();

        let dirty_slot_indices: Vec<usize> = self.dirty_slots.keys().copied().collect();

        let mut any_written = false;
        let mut slots_done: Vec<usize> = Vec::new();
        let mut slots_to_reclaim: Vec<usize> = Vec::new();

        {
            let stage_slice = self.buffers[fif_idx].material_stage_buffer.mapped_slice();

            for &slot in &dirty_slot_indices {
                let info = &self.dirty_slots[&slot];

                if self.slots[slot].is_none() {
                    // slot 已删除：检查回收计时
                    let age = current_frame_id.saturating_sub(info.dirty_frame_id);
                    if age >= fif_count {
                        slots_to_reclaim.push(slot);
                    }
                    continue;
                }

                if !info.fif_dirty[fif_idx] {
                    continue;
                }

                let params = self.slots[slot].as_ref().unwrap();
                stage_slice[slot] = Self::build_gpu_material(params, texture_resolver);
                any_written = true;
            }
        }

        // 更新 dirty 标记（已项 stage_slice borrow 释放）
        for &slot in &dirty_slot_indices {
            if self.slots[slot].is_none() {
                continue;
            }
            let info = match self.dirty_slots.get_mut(&slot) {
                Some(i) => i,
                None => continue,
            };
            if !info.fif_dirty[fif_idx] {
                continue;
            }
            info.fif_dirty[fif_idx] = false;
            if info.fif_dirty.iter().all(|&d| !d) {
                slots_done.push(slot);
            }
        }

        for slot in slots_done {
            self.dirty_slots.remove(&slot);
        }
        for slot in slots_to_reclaim {
            self.dirty_slots.remove(&slot);
            self.free_slots.push(slot);
            log::debug!("MaterialManager: reclaimed slot={}", slot);
        }

        if any_written {
            let buf = &mut self.buffers[fif_idx];
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
        self.slots[slot].as_ref().map(|e| e)
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
