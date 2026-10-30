use ash::{vk, vk::Handle};

use crate::{foundation::debug_messenger::DebugType, gfx::GfxDeviceCtx};

/// Fence 的唯一 owner；提交只借用，调用方等待 GPU 完成后显式销毁。
pub struct GfxFence {
    fence: vk::Fence,
    debug_name: String,
}

impl DebugType for GfxFence {
    fn debug_type_name() -> &'static str {
        "GfxFence"
    }

    fn vk_handle(&self) -> impl vk::Handle {
        self.fence
    }
}

// 创建与销毁
impl GfxFence {
    /// # 参数
    /// * signaled - 是否创建时就 signaled
    pub fn new(ctx: GfxDeviceCtx<'_>, signaled: bool, debug_name: &str) -> Self {
        let gfx_device = ctx.device();
        let debug_name = debug_name.to_owned();
        let fence_flags = if signaled { vk::FenceCreateFlags::SIGNALED } else { vk::FenceCreateFlags::empty() };
        let fence =
            unsafe { gfx_device.create_fence(&vk::FenceCreateInfo::default().flags(fence_flags), None).unwrap() };

        let fence = Self { fence, debug_name };
        gfx_device.set_debug_name(&fence, &fence.debug_name);
        fence
    }
    #[inline]
    pub fn destroy(mut self, ctx: GfxDeviceCtx<'_>) {
        let gfx_device = ctx.device();
        unsafe {
            gfx_device.destroy_fence(self.fence, None);
        }
        self.fence = vk::Fence::null();
    }
}

impl Drop for GfxFence {
    fn drop(&mut self) {
        debug_assert!(self.fence.is_null(), "GfxFence '{}' dropped without explicit destroy", self.debug_name);
    }
}

// 访问器
impl GfxFence {
    #[inline]
    pub fn handle(&self) -> vk::Fence {
        self.fence
    }
}

// 工具函数
impl GfxFence {
    /// 阻塞等待 fence
    #[inline]
    pub fn wait(&self, ctx: GfxDeviceCtx<'_>) {
        let gfx_device = ctx.device();
        unsafe {
            gfx_device.wait_for_fences(std::slice::from_ref(&self.fence), true, u64::MAX).unwrap();
        }
    }

    #[inline]
    pub fn reset(&self, ctx: GfxDeviceCtx<'_>) {
        let gfx_device = ctx.device();
        unsafe {
            gfx_device.reset_fences(std::slice::from_ref(&self.fence)).unwrap();
        }
    }
}
