use ash::vk;
use ash::vk::Handle;

use crate::{foundation::debug_messenger::DebugType, gfx::GfxDeviceCtx};

/// Vulkan semaphore 的唯一所有者。
///
/// 需要共享时传递引用或 raw handle；克隆该类型会让同一个 Vulkan handle 出现多个表面所有者。
pub struct GfxSemaphore {
    semaphore: vk::Semaphore,
    semaphore_type: vk::SemaphoreType,
    debug_name: String,
}

// 创建与销毁
impl GfxSemaphore {
    pub fn new(ctx: GfxDeviceCtx<'_>, debug_name: &str) -> Self {
        Self::new_internal(ctx, None, debug_name)
    }

    pub fn new_timeline(ctx: GfxDeviceCtx<'_>, initial_value: u64, debug_name: &str) -> Self {
        Self::new_internal(ctx, Some(initial_value), debug_name)
    }

    fn new_internal(ctx: GfxDeviceCtx<'_>, initial_value: Option<u64>, debug_name: &str) -> Self {
        let gfx_device = ctx.device();
        let debug_name = debug_name.to_owned();
        let semaphore_type =
            if initial_value.is_some() { vk::SemaphoreType::TIMELINE } else { vk::SemaphoreType::BINARY };
        let mut timeline_type_ci = vk::SemaphoreTypeCreateInfo::default()
            .semaphore_type(semaphore_type)
            .initial_value(initial_value.unwrap_or_default());
        let mut create_info = vk::SemaphoreCreateInfo::default();
        if initial_value.is_some() {
            create_info = create_info.push_next(&mut timeline_type_ci);
        }
        let semaphore = unsafe { gfx_device.create_semaphore(&create_info, None).unwrap() };

        let semaphore = Self {
            semaphore,
            semaphore_type,
            debug_name,
        };
        gfx_device.set_debug_name(&semaphore, &semaphore.debug_name);
        semaphore
    }
    #[inline]
    pub fn destroy(mut self, ctx: GfxDeviceCtx<'_>) {
        if self.semaphore.is_null() {
            return;
        }
        log::trace!(
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

    /// WSI acquire/present 只能使用 binary semaphore；存活与未完成操作仍由 owner 保证。
    pub(crate) fn binary_handle(&self) -> vk::Semaphore {
        assert_eq!(self.semaphore_type, vk::SemaphoreType::BINARY, "WSI requires a binary semaphore");
        self.semaphore
    }
}

// 工具函数
impl GfxSemaphore {
    #[inline]
    pub fn wait_timeline(&self, ctx: GfxDeviceCtx<'_>, timeline_value: u64, timeout_ns: u64) {
        assert_eq!(self.semaphore_type, vk::SemaphoreType::TIMELINE, "binary semaphore cannot be host-waited");
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
