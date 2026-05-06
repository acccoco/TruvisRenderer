use ash::vk;

use crate::{foundation::debug_messenger::DebugType, gfx::Gfx};

/// # 销毁
/// 不应该实现 Semaphore，因为可以 Clone，需要手动 destroy
#[derive(Clone)]
pub struct GfxSemaphore {
    semaphore: vk::Semaphore,
}

// 创建与销毁
impl GfxSemaphore {
    pub fn new(debug_name: &str) -> Self {
        let gfx_device = Gfx::get().gfx_device();
        let semaphore = unsafe { gfx_device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).unwrap() };

        let semaphore = Self { semaphore };
        gfx_device.set_debug_name(&semaphore, debug_name);
        semaphore
    }

    pub fn new_timeline(initial_value: u64, debug_name: &str) -> Self {
        let gfx_device = Gfx::get().gfx_device();
        let mut timeline_type_ci = vk::SemaphoreTypeCreateInfo::default()
            .semaphore_type(vk::SemaphoreType::TIMELINE)
            .initial_value(initial_value);
        let timeline_semaphore_ci = vk::SemaphoreCreateInfo::default().push_next(&mut timeline_type_ci);
        let semaphore = unsafe { gfx_device.create_semaphore(&timeline_semaphore_ci, None).unwrap() };

        let semaphore = Self { semaphore };
        gfx_device.set_debug_name(&semaphore, debug_name);
        semaphore
    }
    #[inline]
    pub fn destroy(self) {
        let gfx_device = Gfx::get().gfx_device();
        unsafe {
            gfx_device.destroy_semaphore(self.semaphore, None);
        }
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
    pub fn wait_timeline(&self, timeline_value: u64, timeout_ns: u64) {
        let gfx_device = Gfx::get().gfx_device();
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
