use std::rc::Rc;

use ash::vk;
use itertools::Itertools;

use crate::{
    commands::{fence::GfxFence, submit_info::GfxSubmitInfo},
    foundation::{debug_messenger::DebugType, device::GfxDevice},
};

#[derive(Clone, Debug)]
pub struct GfxQueueFamily {
    pub name: String,
    pub queue_family_index: u32,
    pub queue_flags: vk::QueueFlags,
    pub queue_count: u32,
}

/// # 销毁
///
/// GfxQueueFamily 在 GfxDevice 销毁时会被销毁
pub struct GfxCommandQueue {
    pub(crate) vk_queue: vk::Queue,
    pub(crate) queue_family: GfxQueueFamily,
    pub(crate) gfx_device: Rc<GfxDevice>,
}
impl DebugType for GfxCommandQueue {
    fn debug_type_name() -> &'static str {
        "GfxQueue"
    }
    fn vk_handle(&self) -> impl vk::Handle {
        self.vk_queue
    }
}

// 访问器
impl GfxCommandQueue {
    #[inline]
    pub fn queue_family(&self) -> &GfxQueueFamily {
        &self.queue_family
    }

    #[inline]
    pub fn handle(&self) -> vk::Queue {
        self.vk_queue
    }
}

// 工具函数
impl GfxCommandQueue {
    pub fn submit(&self, batches: Vec<GfxSubmitInfo>, fence: Option<GfxFence>) {
        unsafe {
            // batches 的存在是有必要的，submit_infos 引用的 batches 的内存
            let batches = batches.iter().map(|b| b.submit_info()).collect_vec();
            self.gfx_device
                .device
                .queue_submit2(self.vk_queue, &batches, fence.map_or(vk::Fence::null(), |f| f.handle()))
                .unwrap()
        }
    }

    /// 根据 specification，vkQueueWaitIdle 应该和 Fence 效率相同
    #[inline]
    pub fn wait_idle(&self) {
        unsafe { self.gfx_device.device.queue_wait_idle(self.vk_queue).unwrap() }
    }
}

// debug 相关命令
impl GfxCommandQueue {
    #[inline]
    pub fn begin_label<S>(&self, label_name: S, label_color: glam::Vec4)
    where
        S: AsRef<str>,
    {
        let name = std::ffi::CString::new(label_name.as_ref()).unwrap();
        unsafe {
            self.gfx_device.debug_utils.queue_begin_debug_utils_label(
                self.vk_queue,
                &vk::DebugUtilsLabelEXT::default().label_name(name.as_c_str()).color(label_color.into()),
            );
        }
    }

    #[inline]
    pub fn end_label(&self) {
        unsafe {
            self.gfx_device.debug_utils.queue_end_debug_utils_label(self.vk_queue);
        }
    }

    #[inline]
    pub fn insert_label<S>(&self, label_name: S, label_color: glam::Vec4)
    where
        S: AsRef<str>,
    {
        let name = std::ffi::CString::new(label_name.as_ref()).unwrap();
        unsafe {
            self.gfx_device.debug_utils.queue_insert_debug_utils_label(
                self.vk_queue,
                &vk::DebugUtilsLabelEXT::default().label_name(name.as_c_str()).color(label_color.into()),
            );
        }
    }
}
