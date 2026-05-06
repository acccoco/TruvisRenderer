use ash::vk;
use ash::vk::Handle;

use crate::{foundation::debug_messenger::DebugType, gfx::GfxDeviceCtx};

/// Vulkan semaphore 的唯一所有者。
///
/// 需要共享时传递引用或 raw handle；克隆该类型会让同一个 Vulkan handle 出现多个表面所有者。
pub struct GfxSemaphore {
    semaphore: vk::Semaphore,
    debug_name: String,
}

// 创建与销毁
impl GfxSemaphore {
    pub fn new(ctx: GfxDeviceCtx<'_>, debug_name: &str) -> Self {
        let gfx_device = ctx.device();
        let semaphore = unsafe { gfx_device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).unwrap() };

        let semaphore = Self {
            semaphore,
            debug_name: debug_name.to_string(),
        };
        gfx_device.set_debug_name(&semaphore, debug_name);
        semaphore
    }

    pub fn new_timeline(ctx: GfxDeviceCtx<'_>, initial_value: u64, debug_name: &str) -> Self {
        let gfx_device = ctx.device();
        let mut timeline_type_ci = vk::SemaphoreTypeCreateInfo::default()
            .semaphore_type(vk::SemaphoreType::TIMELINE)
            .initial_value(initial_value);
        let timeline_semaphore_ci = vk::SemaphoreCreateInfo::default().push_next(&mut timeline_type_ci);
        let semaphore = unsafe { gfx_device.create_semaphore(&timeline_semaphore_ci, None).unwrap() };

        let semaphore = Self {
            semaphore,
            debug_name: debug_name.to_string(),
        };
        gfx_device.set_debug_name(&semaphore, debug_name);
        semaphore
    }
    #[inline]
    pub fn destroy(mut self, ctx: GfxDeviceCtx<'_>) {
        if self.semaphore.is_null() {
            return;
        }
        log::debug!(
            "Destroying GfxSemaphore name={} raw={:#x} reason=shutdown",
            self.debug_name,
            self.semaphore.as_raw()
        );
        let gfx_device = ctx.device();
        unsafe {
            gfx_device.destroy_semaphore(self.semaphore, None);
        }
        self.semaphore = vk::Semaphore::null();
    }
}

// 访问器
impl GfxSemaphore {
    #[inline]
    pub fn handle(&self) -> vk::Semaphore {
        self.semaphore
    }
}

// 工具函数
impl GfxSemaphore {
    #[inline]
    pub fn wait_timeline(&self, ctx: GfxDeviceCtx<'_>, timeline_value: u64, timeout_ns: u64) {
        let gfx_device = ctx.device();
        unsafe {
            let wait_semaphore = [self.semaphore];
            let wait_info = vk::SemaphoreWaitInfo::default()
                .semaphores(&wait_semaphore)
                .values(std::slice::from_ref(&timeline_value));
            gfx_device.wait_semaphores(&wait_info, timeout_ns).unwrap();
        }
    }
}

impl DebugType for GfxSemaphore {
    fn debug_type_name() -> &'static str {
        "GfxSemaphore"
    }

    fn vk_handle(&self) -> impl vk::Handle {
        self.semaphore
    }
}

impl Drop for GfxSemaphore {
    fn drop(&mut self) {
        debug_assert!(
            self.semaphore.is_null(),
            "GfxSemaphore '{}' dropped without explicit owner destroy",
            self.debug_name
        );
    }
}
