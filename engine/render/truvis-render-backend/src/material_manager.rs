use std::collections::{HashMap, HashSet};

use ash::vk;
use slotmap::SlotMap;
use slotmap::new_key_type;

use truvis_asset::handle::{AssetTextureHandle, MaterialData};
use truvis_gfx::commands::barrier::{GfxBarrierMask, GfxBufferBarrier};
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_render_interface::bindless_manager::BindlessSrvHandle;
use truvis_render_interface::frame_counter::{FrameCounter, FrameToken};
use truvis_render_interface::pipeline_settings::FrameLabel;
use truvis_shader_binding::gpu;

new_key_type! {
    /// backend 私有的 GPU material handle。
    ///
    /// 它与 `AssetMaterialHandle` 分离，用于保持 shader 可见 material slot 的稳定性。
    pub struct GpuMaterialHandle;
}

/// MaterialManager 使用的 CPU 侧材质参数。
///
/// texture 字段使用 `AssetTextureHandle`，支持异步加载和 bindless 绑定。
#[derive(Clone, PartialEq)]
pub struct ManagedMaterialParams {
    pub base_color: glam::Vec4,
    pub emissive: glam::Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub opaque: f32,

    pub diffuse_texture: Option<AssetTextureHandle>,
    pub normal_texture: Option<AssetTextureHandle>,
}

impl From<&MaterialData> for ManagedMaterialParams {
    fn from(mat: &MaterialData) -> Self {
        Self {
            base_color: mat.base_color,
            emissive: mat.emissive,
            metallic: mat.metallic,
            roughness: mat.roughness,
            opaque: mat.opaque,
            diffuse_texture: mat.diffuse_texture,
            normal_texture: mat.normal_texture,
        }
    }
}

impl Default for ManagedMaterialParams {
    fn default() -> Self {
        Self {
            base_color: glam::Vec4::ONE,
            emissive: glam::Vec4::ZERO,
            metallic: 0.0,
            roughness: 0.5,
            opaque: 1.0,
            diffuse_texture: None,
            normal_texture: None,
        }
    }
}

const MAX_MATERIAL_COUNT: usize = 1024;

#[derive(Clone, Copy)]
pub struct TextureBinding {
    pub srv_handle: BindlessSrvHandle,
    pub sampler: gpu::ESamplerType,
}

impl TextureBinding {
    /// 构造 shader 可安全读取的空 texture binding。
    ///
    /// 该值用于“材质没有贴图”场景；“贴图存在但未 ready”由 `TextureResolver`
    /// 返回 fallback binding 处理。
    pub fn null() -> Self {
        Self {
            srv_handle: BindlessSrvHandle::null(),
            sampler: gpu::ESamplerType_LinearRepeat,
        }
    }
}

/// 纹理 ready 状态与 shader binding 查询接口。
///
/// 由渲染侧纹理上传/绑定缓存实现，避免 scene 直接耦合 AssetHub 或 BindlessManager。
pub trait TextureResolver {
    /// texture 是否已经拥有真实 GPU image/view/bindless binding。
    fn is_texture_ready(&self, handle: AssetTextureHandle) -> bool;

    /// 获取可渲染的 texture binding；未就绪时由实现返回 fallback。
    fn resolve_texture(&self, handle: AssetTextureHandle) -> TextureBinding;
}

/// 单个 material slot 在 dirty 列表中的 FIF 写入与回收状态。
struct SlotDirtyInfo {
    /// 各 FIF buffer 是否需要更新；true 表示需要写入该帧对应的 GPU buffer。
    fif_dirty: [bool; FrameCounter::fif_count()],
    /// 本次 dirty 或 unregister 发生时的 frame id，用于延迟回收计时。
    dirty_frame_id: u64,
}

/// 单个 FIF frame label 对应的材质 GPU buffer 与 staging buffer。
struct MaterialBuffers {
    /// Device-local SSBO，shader 通过 scene root buffer 中的 device address 读取。
    material_buffer: GfxStructuredBuffer<gpu::PBRMaterial>,
    /// Host-mapped staging buffer，CPU 写入后在 prepare 命令中复制到 SSBO。
    material_stage_buffer: GfxStructuredBuffer<gpu::PBRMaterial>,
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
                format!("MaterialManager::material_buffer-{}", frame_label),
            ),
            material_stage_buffer: GfxStructuredBuffer::new_stage_buffer(
                ctx,
                MAX_MATERIAL_COUNT,
                format!("MaterialManager::material_stage_buffer-{}", frame_label),
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
/// 将 CPU 材质参数、GPU slot 映射、dirty 状态和增量上传逻辑聚合为独立模块，
/// 而非分散在 SceneManager（CPU 数据）和 GpuScene（GPU buffer）之间。
/// 这与 `BindlessManager` 的设计模式一致——每种 GPU 资源由专门的 Manager 自治管理。
/// 在 backend 分层中，它是材质数据从 asset 世界进入 shader 可见 buffer 的最后一道 owner。
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
pub struct MaterialManager {
    /// 核心映射：GpuMaterialHandle -> shader 可见 material buffer slot。
    handle_to_slot: SlotMap<GpuMaterialHandle, usize>,

    /// slot 数据：index = GPU buffer 中的位置；None 表示已 unregister、等待延迟回收。
    slots: Vec<Option<ManagedMaterialParams>>,

    /// 可立即分配的 slot。被删除的 slot 必须跨过 FIF 窗口后才能回到这里。
    free_slots: Vec<usize>,

    /// dirty 列表：slot index -> SlotDirtyInfo，记录每个 FIF buffer 是否还需要补写该 slot。
    dirty_slots: HashMap<usize, SlotDirtyInfo>,

    /// 等待 texture 就绪的材质 handle；ready 后会重新 dirty 所有 FIF buffer。
    pending_texture_ready: HashSet<GpuMaterialHandle>,

    /// FIF 套 GPU buffer，避免 CPU 覆盖 GPU 仍在读取的 material buffer。
    buffers: [MaterialBuffers; FrameCounter::fif_count()],

    frame_token: FrameToken,
}

// 创建与初始化
impl MaterialManager {
    /// 创建 FIF 套材质 buffer，并初始化可分配 slot 池。
    pub fn new(ctx: GfxResourceCtx<'_>, frame_token: FrameToken) -> Self {
        let free_slots: Vec<usize> = (0..MAX_MATERIAL_COUNT).rev().collect();
        Self {
            handle_to_slot: SlotMap::with_key(),
            slots: (0..MAX_MATERIAL_COUNT).map(|_| None).collect(),
            free_slots,
            dirty_slots: HashMap::new(),
            pending_texture_ready: HashSet::new(),
            buffers: FrameCounter::frame_labes().map(|frame_label| MaterialBuffers::new(ctx, frame_label)),
            frame_token,
        }
    }
}

// 销毁
impl MaterialManager {
    /// 销毁所有 FIF material buffer。
    pub fn destroy(mut self, ctx: GfxResourceCtx<'_>) {
        for buffer in &mut self.buffers {
            buffer.destroy_mut(ctx);
        }
    }
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
    pub fn register(&mut self, params: ManagedMaterialParams) -> GpuMaterialHandle {
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
            // 注册时可能 texture 尚未上传完成。材质 slot 先用 fallback/null 可见，
            // texture ready 后再通过 update 触发全 FIF 重新上传。
            self.pending_texture_ready.insert(handle);
        }

        log::trace!("MaterialManager: register slot={} handle={:?}", slot, handle);
        handle
    }

    /// 更新已注册材质的参数
    ///
    /// 会标记所有 FIF buffer 为 dirty，后续帧会逐个更新。
    pub fn update_params(&mut self, handle: GpuMaterialHandle, params: ManagedMaterialParams) {
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
            // 参数变化可能换成新的 texture handle，因此需要重新进入 ready 检测集合。
            self.pending_texture_ready.insert(handle);
        } else {
            self.pending_texture_ready.remove(&handle);
        }
    }

    /// 移除材质，延迟回收 slot
    ///
    /// slot 内容不再上传，但 slot index 会继续保留至少 `FIF_COUNT` 帧，避免在飞命令仍用旧 index
    /// 访问 material buffer 时被新材质复用。
    pub fn unregister(&mut self, handle: GpuMaterialHandle) {
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
    /// 帧开始时调用，更新后续 dirty/回收判断使用的 frame token。
    pub fn begin_frame(&mut self, frame_token: FrameToken) {
        // 实际回收发生在 upload 中，因为回收判断需要和当前 FIF dirty 状态处理保持同一处。
        self.frame_token = frame_token;
    }

    /// 检查 texture 异步加载状态，尝试新增 dirty 标记
    ///
    /// 材质可以先用 fallback/null texture 上传；当 resolver 报告真实 texture ready 时，
    /// 再把所有 FIF buffer 标记为 dirty，让 shader 在后续帧看到真实绑定。
    pub fn update(&mut self, texture_resolver: &dyn TextureResolver) {
        let frame_id = self.frame_token.frame_id();

        let now_ready: Vec<GpuMaterialHandle> = self
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
            log::trace!("MaterialManager: textures ready handle={:?} slot={}; dirty all FIF buffers", handle, slot);
        }
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
            // stage buffer 的可变借用范围刻意限制在这个 block 内；后续需要再次可变访问
            // dirty_slots 和 buffer owner 来更新状态并提交 copy/barrier。
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

        // 更新 dirty 标记（此时 stage_slice borrow 已释放）。
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
            // 当前实现按整段 material buffer copy，保证简单可靠；dirty slot 只决定是否需要
            // 发起本 FIF 的 copy，后续可再细化为 region copy。
            let buf = &mut self.buffers[fif_idx];
            Self::flush_copy_and_barrier(
                ctx,
                cmd,
                &mut buf.material_stage_buffer,
                &mut buf.material_buffer,
                barrier_mask,
            );
        }
    }
}

// 访问器
impl MaterialManager {
    /// 获取材质在 GPU buffer 中的 slot index
    #[inline]
    pub fn get_slot_index(&self, handle: GpuMaterialHandle) -> Option<usize> {
        self.handle_to_slot.get(handle).copied()
    }

    /// 获取指定帧的 material buffer device address
    #[inline]
    pub fn material_buffer_device_address(&self, frame_label: FrameLabel) -> vk::DeviceAddress {
        self.buffers[*frame_label].material_buffer.device_address()
    }
}

// 内部工具方法
impl MaterialManager {
    /// 判断材质引用的所有 texture 是否已经能解析为真实 shader binding。
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
    /// 将 CPU 材质参数转换为 shader 读取的 packed GPU 数据。
    ///
    /// texture handle 在这里通过 resolver 转成 bindless SRV index；resolver 保证未 ready
    /// 的 texture 也会返回 fallback，因此 GPU 数据不会包含悬空句柄。
    fn build_gpu_material(params: &ManagedMaterialParams, resolver: &dyn TextureResolver) -> gpu::PBRMaterial {
        let diffuse_binding =
            params.diffuse_texture.map(|h| resolver.resolve_texture(h)).unwrap_or(TextureBinding::null());
        let normal_binding =
            params.normal_texture.map(|h| resolver.resolve_texture(h)).unwrap_or(TextureBinding::null());

        gpu::PBRMaterial {
            base_color: params.base_color.truncate().into(),
            emissive: params.emissive.truncate().into(),
            metallic: params.metallic,
            roughness: params.roughness,
            diffuse_map: diffuse_binding.srv_handle.0,
            diffuse_map_sampler_type: diffuse_binding.sampler,
            normal_map: normal_binding.srv_handle.0,
            normal_map_sampler_type: normal_binding.sampler,
            opaque: params.opaque,
            _padding_1: Default::default(),
            _padding_2: Default::default(),
            _padding_3: Default::default(),
        }
    }

    // TODO 可以细化更新 regions
    /// 将当前 staging material buffer 刷新、复制到 device buffer，并建立 shader-read barrier。
    ///
    /// `barrier_mask` 来自 prepare pipeline，和 scene/per-frame buffer 使用同一套可见性约定。
    fn flush_copy_and_barrier(
        ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        stage_buffer: &mut GfxStructuredBuffer<gpu::PBRMaterial>,
        dst_buffer: &mut GfxStructuredBuffer<gpu::PBRMaterial>,
        barrier_mask: GfxBarrierMask,
    ) {
        let buffer_size = stage_buffer.size();
        stage_buffer.flush(ctx, 0, buffer_size);
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
