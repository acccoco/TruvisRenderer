use crate::frame_counter::{FrameCounter, FrameToken};
use crate::gfx_resource_manager::GfxResourceManager;
use crate::global_descriptor_sets::{BindlessDescriptorBinding, GlobalDescriptorSets};
use crate::handles::GfxImageViewHandle;
use ash::vk;
use image::Frame;
use slotmap::{Key, SecondaryMap};
use std::collections::HashMap;
use truvis_gfx::{gfx::Gfx, utilities::descriptor_cursor::GfxDescriptorCursor};
use truvis_shader_binding::gpu;

/// 每个 bindless 类型（SRV/UAV）允许的最大 slot 数，须与 descriptor layout 的 count 对齐
const MAX_BINDLESS_COUNT: usize = 128;

#[derive(Copy, Clone)]
pub struct BindlessUavHandle(pub gpu::UavHandle);
impl BindlessUavHandle {
    #[inline]
    pub fn new(index: usize) -> Self {
        Self(gpu::UavHandle { index: index as i32 })
    }
    #[inline]
    pub fn null() -> Self {
        Self(gpu::UavHandle {
            index: gpu::INVALID_TEX_ID,
        })
    }
    #[inline]
    pub fn index(&self) -> usize {
        self.0.index as usize
    }
}
impl Default for BindlessUavHandle {
    fn default() -> Self {
        Self::null()
    }
}

#[derive(Copy, Clone)]
pub struct BindlessSrvHandle(pub gpu::SrvHandle);
impl BindlessSrvHandle {
    #[inline]
    pub fn new(index: usize) -> Self {
        Self(gpu::SrvHandle { index: index as i32 })
    }
    #[inline]
    pub fn null() -> Self {
        Self(gpu::SrvHandle {
            index: gpu::INVALID_TEX_ID,
        })
    }
    #[inline]
    pub fn index(&self) -> usize {
        self.0.index as usize
    }
}
impl Default for BindlessSrvHandle {
    fn default() -> Self {
        Self::null()
    }
}

/// Bindless 描述符管理器
///
/// - 只允许 add 和 remove 操作，不支持 update 操作
///
/// # slot 稳定性
/// - 使用单套 descriptor set（配合 `UPDATE_UNUSED_WHILE_PENDING_BIT`），slot 是稳定的：
///
/// # 更新与回收
/// - `dirty` map 的 value 记录最后写入时的 `frame_id`，用于延迟回收 slot
/// - add 可以立即写入 descriptor，因为可以确保这个 slot 不会被 GPU 同时访问
///
/// # 安全性
/// - `UPDATE_UNUSED_WHILE_PENDING_BIT` 允许 CPU 在有 in-flight 命令时更新 descriptor，
///     只要该 slot 未被这些命令动态访问。
/// - slot 回收机制保证：slot 归还 free_list 时，所有引用它的 in-flight 命令已完成。
/// - 仅支持 add 和 remove 操作，不支持 update，因此可以确保所有的 dirty slot 都不会被 GPU 同时访问。
pub struct BindlessManager {
    // 核心结构：index = bindless shader slot
    srvs_slots: Vec<Option<GfxImageViewHandle>>,
    uavs_slots: Vec<Option<GfxImageViewHandle>>,

    // 空闲 slot 池（降序存放，pop 返回较小 slot）
    srvs_free_slots: Vec<usize>,
    uavs_free_slots: Vec<usize>,

    // 逆向映射：handle → slot（辅助查询）
    srvs_handle_to_slot: SecondaryMap<GfxImageViewHandle, usize>,
    uavs_handle_to_slot: SecondaryMap<GfxImageViewHandle, usize>,

    /// dirty 列表：key=slot，value=最后修改时的 frame_id
    dirty_srvs: HashMap<usize, u64>,
    dirty_uavs: HashMap<usize, u64>,

    /// 当前帧的 token，用于计算 dirty 条目的 age
    ///
    /// 在 begin frame 时传入
    frame_token: FrameToken,
}

// new & init
impl BindlessManager {
    pub fn new(frame_token: FrameToken) -> Self {
        // 降序填充，使得 pop() 优先分配较小 slot（方便调试观察）
        let free_slots: Vec<usize> = (0..MAX_BINDLESS_COUNT).rev().collect();
        Self {
            srvs_slots: vec![None; MAX_BINDLESS_COUNT],
            uavs_slots: vec![None; MAX_BINDLESS_COUNT],
            srvs_free_slots: free_slots.clone(),
            uavs_free_slots: free_slots,
            srvs_handle_to_slot: SecondaryMap::new(),
            uavs_handle_to_slot: SecondaryMap::new(),
            dirty_srvs: HashMap::new(),
            dirty_uavs: HashMap::new(),
            frame_token,
        }
    }
}

// destroy
impl BindlessManager {
    pub fn destroy(self) {}
}
impl Drop for BindlessManager {
    fn drop(&mut self) {
        log::info!("Dropping BindlessManager");
    }
}

// update
impl BindlessManager {
    pub fn begin_frame(&mut self, frame_token: FrameToken) {
        self.frame_token = frame_token;
    }

    /// # Phase: Before Render
    ///
    /// 增量更新 bindless descriptor set：
    /// - 对新注册的 slot（`slots[slot] = Some`），写入 descriptor 后立即从 dirty 移除
    /// - 对已注销的 slot（`slots[slot] = None`），等 age >= FIF_COUNT 后归还 free_list
    pub fn prepare_render_data(
        &mut self,
        gfx_resource_manager: &GfxResourceManager,
        render_descriptor_sets: &GlobalDescriptorSets,
    ) {
        let _span = tracy_client::span!("BindlessManager::prepare_render_data");

        let bindless_set = render_descriptor_sets.bindless_set().handle();
        let fif = FrameCounter::fif_count() as u64;
        let mut writes = Vec::new();

        // 处理 SRV dirty 条目
        let mut srvs_to_remove: Vec<usize> = Vec::new();
        let mut srvs_to_reclaim: Vec<usize> = Vec::new();
        for (&slot, &dirty_frame_id) in &self.dirty_srvs {
            match self.srvs_slots[slot] {
                Some(view_handle) => {
                    // 新注册的 slot：写入 descriptor，立即从 dirty 移除
                    let image_view = gfx_resource_manager.get_image_view(view_handle).unwrap();
                    let image_info = vk::DescriptorImageInfo::default()
                        .image_view(image_view.handle())
                        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
                    writes.push(BindlessDescriptorBinding::srvs().write_image(
                        bindless_set,
                        slot as u32,
                        vec![image_info],
                    ));
                    srvs_to_remove.push(slot);
                }
                None => {
                    // 已注销的 slot：等 GPU 不再访问后归还
                    let age = self.frame_token.frame_id().saturating_sub(dirty_frame_id);
                    if age >= fif {
                        srvs_to_remove.push(slot);
                        srvs_to_reclaim.push(slot);
                    }
                }
            }
        }

        // 处理 UAV dirty 条目
        let mut uavs_to_remove: Vec<usize> = Vec::new();
        let mut uavs_to_reclaim: Vec<usize> = Vec::new();
        for (&slot, &dirty_frame_id) in &self.dirty_uavs {
            match self.uavs_slots[slot] {
                Some(view_handle) => {
                    let image_view = gfx_resource_manager.get_image_view(view_handle).unwrap();
                    let image_info = vk::DescriptorImageInfo::default()
                        .image_view(image_view.handle())
                        .image_layout(vk::ImageLayout::GENERAL);
                    writes.push(BindlessDescriptorBinding::uavs().write_image(
                        bindless_set,
                        slot as u32,
                        vec![image_info],
                    ));
                    uavs_to_remove.push(slot);
                }
                None => {
                    let age = self.frame_token.frame_id().saturating_sub(dirty_frame_id);
                    if age >= fif {
                        uavs_to_remove.push(slot);
                        uavs_to_reclaim.push(slot);
                    }
                }
            }
        }

        // 提交所有 descriptor 写入
        if !writes.is_empty() {
            Gfx::get().gfx_device().write_descriptor_sets(&writes);
        }

        // 清除已处理的 dirty 条目，归还已回收的 slot
        for slot in srvs_to_remove {
            self.dirty_srvs.remove(&slot);
        }
        for slot in srvs_to_reclaim {
            self.srvs_free_slots.push(slot);
        }
        for slot in uavs_to_remove {
            self.dirty_uavs.remove(&slot);
        }
        for slot in uavs_to_reclaim {
            self.uavs_free_slots.push(slot);
        }
    }
}

// UAV
impl BindlessManager {
    pub fn register_uav(&mut self, image_view_handle: GfxImageViewHandle) {
        debug_assert!(!image_view_handle.is_null());
        if self.uavs_handle_to_slot.contains_key(image_view_handle) {
            log::error!("UAV handle {:?} is already registered", image_view_handle);
            return;
        }
        let slot = self.uavs_free_slots.pop().expect("Bindless UAV slots exhausted");
        self.uavs_slots[slot] = Some(image_view_handle);
        self.uavs_handle_to_slot.insert(image_view_handle, slot);
        self.dirty_uavs.insert(slot, self.frame_token.frame_id());
    }

    pub fn unregister_uav(&mut self, image_view_handle: GfxImageViewHandle) {
        debug_assert!(!image_view_handle.is_null());
        let slot = self.uavs_handle_to_slot.remove(image_view_handle).unwrap();
        self.uavs_slots[slot] = None;
        // 不立即归还 free_list，等 dirty 清除（age >= FIF_COUNT）后再归还
        self.dirty_uavs.insert(slot, self.frame_token.frame_id());
    }

    #[inline]
    pub fn get_shader_uav_handle(&self, image_view_handle: GfxImageViewHandle) -> BindlessUavHandle {
        debug_assert!(!image_view_handle.is_null());
        let slot = *self.uavs_handle_to_slot.get(image_view_handle).unwrap();
        BindlessUavHandle::new(slot)
    }
}

// SRV
impl BindlessManager {
    pub fn register_srv(&mut self, image_view_handle: GfxImageViewHandle) {
        debug_assert!(!image_view_handle.is_null());
        if self.srvs_handle_to_slot.contains_key(image_view_handle) {
            log::error!("SRV handle {:?} is already registered", image_view_handle);
            return;
        }
        let slot = self.srvs_free_slots.pop().expect("Bindless SRV slots exhausted");
        self.srvs_slots[slot] = Some(image_view_handle);
        self.srvs_handle_to_slot.insert(image_view_handle, slot);
        self.dirty_srvs.insert(slot, self.frame_token.frame_id());
    }

    pub fn unregister_srv(&mut self, image_view_handle: GfxImageViewHandle) {
        debug_assert!(!image_view_handle.is_null());
        let slot = self.srvs_handle_to_slot.remove(image_view_handle).unwrap();
        self.srvs_slots[slot] = None;
        // 不立即归还 free_list，等 dirty 清除（age >= FIF_COUNT）后再归还
        self.dirty_srvs.insert(slot, self.frame_token.frame_id());
    }

    #[inline]
    pub fn get_shader_srv_handle(&self, image_view_handle: GfxImageViewHandle) -> BindlessSrvHandle {
        debug_assert!(!image_view_handle.is_null());
        let slot = *self.srvs_handle_to_slot.get(image_view_handle).unwrap();
        BindlessSrvHandle::new(slot)
    }
}
