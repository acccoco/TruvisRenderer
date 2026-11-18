use std::collections::HashMap;

use ash::vk;
use slotmap::{Key, SecondaryMap};

use truvis_gfx::{gfx::GfxDeviceCtx, utilities::descriptor_cursor::GfxDescriptorCursor};
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_render_foundation::handles::GfxImageViewHandle;
use truvis_shader_binding::gpu;

use crate::bindings::descriptor_bindings::{BindlessDescriptorBinding, BindlessDescriptorTarget};
use crate::resources::gfx_resource_registry::GfxResourceRegistry;

#[derive(Copy, Clone)]
pub struct BindlessSrvHandle(pub gpu::engine::bindless::SrvHandle);
impl BindlessSrvHandle {
    #[inline]
    pub fn new(index: usize) -> Self {
        Self(gpu::engine::bindless::SrvHandle {
            index: i32::try_from(index).expect("bindless SRV index exceeds shader integer range"),
        })
    }
    #[inline]
    pub fn null() -> Self {
        Self(gpu::engine::bindless::SrvHandle {
            index: gpu::engine::bindless::INVALID_TEX_ID,
        })
    }
    #[inline]
    pub fn index(&self) -> usize {
        usize::try_from(self.0.index).expect("null bindless SRV has no slot index")
    }
}
impl Default for BindlessSrvHandle {
    fn default() -> Self {
        Self::null()
    }
}

/// 数据驱动 sampled image 的全局 bindless 描述符管理器。
///
/// 固定管线 image 由各 pass 的 local descriptor 绑定；这里仅保存 Material/Scene shader record
/// 中需要稳定 slot 的 asset texture、sky 及 fallback SRV。
///
/// # slot 稳定性
/// - 使用单套 descriptor set（配合 `UPDATE_UNUSED_WHILE_PENDING_BIT`），slot 是稳定的：
///
/// # 更新与回收
/// - `dirty` map 的 value 记录最后修改时的 `frame_id`，用于延迟回收 slot
/// - add 可以立即写入 descriptor，因为可以确保这个 slot 不会被 GPU 同时访问
///
/// # 安全性
/// - `UPDATE_UNUSED_WHILE_PENDING_BIT` 允许 CPU 在有 in-flight 命令时更新 descriptor，
///   只要该 slot 未被这些命令动态访问。
/// - slot 回收机制保证：slot 归还 free_list 时，所有引用它的 in-flight 命令已完成。
/// - 仅支持 add 和 remove 操作，不支持 update，因此可以确保所有的 dirty slot 都不会被 GPU 同时访问。
pub struct BindlessManager {
    // 核心结构：index = bindless shader slot
    srvs_slots: Vec<Option<GfxImageViewHandle>>,

    // 空闲 slot 池（降序存放，pop 返回较小 slot）
    srvs_free_slots: Vec<usize>,

    // 逆向映射：handle → slot（辅助查询）
    srvs_handle_to_slot: SecondaryMap<GfxImageViewHandle, usize>,

    /// Some slot 等待写入，None slot 等待退役；共用修改帧号，避免维护平行身份表。
    dirty_srvs: HashMap<usize, u64>,

    /// 当前帧的 token，用于计算 dirty 条目的 age
    ///
    /// 在 begin frame 时传入
    current_frame_id: u64,
}

// 创建与初始化
impl BindlessManager {
    pub fn new(current_frame_id: u64) -> Self {
        let _span = tracy_client::span!("BindlessManager::new");

        let bindless_count = BindlessDescriptorBinding::descriptor_count();
        // 降序填充，使得 pop() 优先分配较小 slot（方便调试观察）
        let free_slots: Vec<usize> = (0..bindless_count).rev().collect();
        Self {
            srvs_slots: vec![None; bindless_count],
            srvs_free_slots: free_slots,
            srvs_handle_to_slot: SecondaryMap::new(),
            dirty_srvs: HashMap::new(),
            current_frame_id,
        }
    }
}

// 更新
impl BindlessManager {
    /// 调用前须完成 current_frame_id-FIF 的 timeline 等待；先归还到期 slot，再允许本帧注册。
    pub fn begin_frame(&mut self, current_frame_id: u64) {
        self.current_frame_id = current_frame_id;
        self.dirty_srvs.retain(|&slot, dirty_frame_id| {
            if self.srvs_slots[slot].is_none()
                && current_frame_id.checked_sub(*dirty_frame_id).is_some_and(|age| age >= FrameLabel::COUNT as u64)
            {
                self.srvs_free_slots.push(slot);
                false
            } else {
                true
            }
        });
    }

    /// # 阶段：Before Render
    ///
    /// 增量更新 bindless descriptor set：
    /// - 对新注册的 slot（`slots[slot] = Some`），写入 descriptor 后立即从 dirty 移除
    /// - 已注销 slot 的到期回收由 begin_frame 完成，不能延迟到本帧注册之后
    pub fn prepare_render_data(
        &mut self,
        ctx: GfxDeviceCtx<'_>,
        gfx_resource_registry: &GfxResourceRegistry,
        bindless_target: BindlessDescriptorTarget,
    ) {
        let _span = tracy_client::span!("BindlessManager::prepare_render_data");

        let bindless_set = bindless_target.set;
        let mut writes = Vec::new();

        // 处理 SRV dirty 条目
        let mut srvs_to_remove: Vec<usize> = Vec::new();
        for &slot in self.dirty_srvs.keys() {
            match self.srvs_slots[slot] {
                Some(view_handle) => {
                    // 新注册的 slot：写入 descriptor，立即从 dirty 移除
                    let image_view = gfx_resource_registry.get_image_view(view_handle).unwrap();
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
                None => (),
            }
        }

        // 提交所有 descriptor 写入
        if !writes.is_empty() {
            ctx.device().write_descriptor_sets(&writes);
        }

        // 只有写入完成才撤销 dirty；失败时保留原条目，不提前交付。
        for slot in srvs_to_remove {
            self.dirty_srvs.remove(&slot);
        }
    }
}

// SRV 资源
impl BindlessManager {
    pub fn register_srv(&mut self, image_view_handle: GfxImageViewHandle) {
        assert!(!image_view_handle.is_null(), "cannot register a null image view");
        if self.srvs_handle_to_slot.contains_key(image_view_handle) {
            log::error!("SRV handle {:?} is already registered", image_view_handle);
            return;
        }
        let slot = self.srvs_free_slots.pop().expect("Bindless SRV slots exhausted");
        self.srvs_slots[slot] = Some(image_view_handle);
        self.srvs_handle_to_slot.insert(image_view_handle, slot);
        self.dirty_srvs.insert(slot, self.current_frame_id);
    }

    pub fn unregister_srv(&mut self, image_view_handle: GfxImageViewHandle) {
        assert!(!image_view_handle.is_null(), "cannot unregister a null image view");
        let slot = self.srvs_handle_to_slot.remove(image_view_handle).unwrap();
        self.srvs_slots[slot] = None;
        // 不立即归还 free_list，等 dirty 清除（age >= FIF_COUNT）后再归还
        self.dirty_srvs.insert(slot, self.current_frame_id);
    }

    #[inline]
    pub fn get_shader_srv_handle(&self, image_view_handle: GfxImageViewHandle) -> BindlessSrvHandle {
        assert!(!image_view_handle.is_null(), "cannot query a null image view");
        let slot = *self.srvs_handle_to_slot.get(image_view_handle).unwrap();
        BindlessSrvHandle::new(slot)
    }
}
