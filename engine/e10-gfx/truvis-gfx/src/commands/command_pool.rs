use ash::vk;
use ash::vk::Handle;

use crate::commands::command_buffer::GfxCommandBuffer;
use crate::{commands::command_queue::GfxQueueFamily, foundation::debug_messenger::DebugType, gfx::GfxDeviceCtx};

/// command pool 是和 queue family 绑定的，而不是和 queue 绑定的
pub struct GfxCommandPool {
    handle: vk::CommandPool,
    _queue_family: GfxQueueFamily,

    debug_name: String,
}
// 创建与初始化
impl GfxCommandPool {
    #[inline]
    pub fn new(
        ctx: GfxDeviceCtx<'_>,
        queue_family: GfxQueueFamily,
        flags: vk::CommandPoolCreateFlags,
        debug_name: &str,
    ) -> Self {
        Self::new_internal(ctx.device_rc(), queue_family, flags, debug_name)
    }

    /// 内部构造函数，用于 `Gfx` root owner 初始化自身临时 command pool。
    /// 这里直接接收 device owner，避免在 root owner 尚未构造完成时反向借用 `Gfx` Ctx。
    #[inline]
    pub(crate) fn new_internal(
        gfx_device: std::rc::Rc<crate::foundation::device::GfxDevice>,
        queue_family: GfxQueueFamily,
        flags: vk::CommandPoolCreateFlags,
        debug_name: &str,
    ) -> Self {
        let pool = unsafe {
            gfx_device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default()
                        .queue_family_index(queue_family.queue_family_index)
                        .flags(flags),
                    None,
                )
                .unwrap()
        };

        let command_pool = Self {
            handle: pool,
            _queue_family: queue_family,
            debug_name: debug_name.to_string(),
        };
        gfx_device.set_debug_name(&command_pool, debug_name);
        command_pool
    }
}
// 销毁
impl GfxCommandPool {
    pub fn destroy(&mut self, ctx: GfxDeviceCtx<'_>) {
        self.release(ctx.device());
    }

    pub fn destroy_internal(mut self, gfx_device: &crate::foundation::device::GfxDevice) {
        self.release(gfx_device);
    }

    fn release(&mut self, gfx_device: &crate::foundation::device::GfxDevice) {
        if self.handle.is_null() {
            return;
        }
        unsafe {
            gfx_device.destroy_command_pool(self.handle, None);
        }
        self.handle = vk::CommandPool::null();
    }
}
// 访问器
impl GfxCommandPool {
    /// 访问器
    #[inline]
    pub fn handle(&self) -> vk::CommandPool {
        assert!(!self.handle.is_null(), "command pool has been destroyed");
        self.handle
    }
}
// 工具函数
impl GfxCommandPool {
    /// 这个调用并不会释放资源，而是将 pool 内的 command buffer 设置到初始状态
    ///
    /// reset 之后，pool 内的 command buffer 又可以重新录制命令
    pub fn reset_command_pool(&self, ctx: GfxDeviceCtx<'_>) {
        let gfx_device = ctx.device();
        unsafe {
            gfx_device.reset_command_pool(self.handle(), vk::CommandPoolResetFlags::RELEASE_RESOURCES).unwrap();
        }
    }

    /// 释放 command buffer
    ///
    /// owner 必须先确认 GPU 完成，并撤销全部外部 clone；检查来源不能撤销已经复制的 handle。
    pub fn free_command_buffers(&self, ctx: GfxDeviceCtx<'_>, command_buffers: Vec<GfxCommandBuffer>) {
        let pool = self.handle();
        let mut command_buffer_handles = Vec::with_capacity(command_buffers.len());
        let mut unique = std::collections::HashSet::with_capacity(command_buffers.len());
        for command in &command_buffers {
            command.validate_pool(pool, ctx.device());
            let handle = command.vk_handle();
            assert!(unique.insert(handle), "duplicate command buffer in free batch");
            command_buffer_handles.push(handle);
        }
        if command_buffer_handles.is_empty() {
            return;
        }
        unsafe {
            ctx.device().free_command_buffers(pool, &command_buffer_handles);
        }
    }
}
impl DebugType for GfxCommandPool {
    fn debug_type_name() -> &'static str {
        "GfxCommandPool"
    }

    fn vk_handle(&self) -> impl vk::Handle {
        self.handle()
    }
}
impl Drop for GfxCommandPool {
    fn drop(&mut self) {
        debug_assert!(self.handle.is_null(), "CommandPool must be destroyed manually.");
        log::info!("Dropping CommandPool: {}", self.debug_name);
    }
}
