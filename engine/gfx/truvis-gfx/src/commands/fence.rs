use ash::vk;

use crate::{foundation::debug_messenger::DebugType, gfx::GfxDeviceCtx};

/// # 销毁
/// 不应该实现 Fence，因为可以 Clone，需要手动 destroy
#[derive(Clone)]
pub struct GfxFence {
    fence: vk::Fence,
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
        let fence_flags = if signaled { vk::FenceCreateFlags::SIGNALED } else { vk::FenceCreateFlags::empty() };
        let fence =
            unsafe { gfx_device.create_fence(&vk::FenceCreateInfo::default().flags(fence_flags), None).unwrap() };

        let fence = Self { fence };
        gfx_device.set_debug_name(&fence, debug_name);
        fence
    }
    #[inline]
    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        let gfx_device = ctx.device();
        unsafe {
            gfx_device.destroy_fence(self.fence, None);
        }
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
