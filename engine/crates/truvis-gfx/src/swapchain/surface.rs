use ash::vk;

use crate::foundation::debug_messenger::DebugType;
use crate::gfx::Gfx;

pub struct GfxSurface {
    pub(crate) handle: vk::SurfaceKHR,
    pub(crate) pf: ash::khr::surface::Instance,
}

impl GfxSurface {
    pub fn new(
        raw_display_handle: raw_window_handle::RawDisplayHandle,
        raw_window_handle: raw_window_handle::RawWindowHandle,
    ) -> Self {
        let gfx_core = &Gfx::get().gfx_core;
        let surface_pf = ash::khr::surface::Instance::new(&gfx_core.vk_entry, &gfx_core.instance.ash_instance);

        let surface = unsafe {
            ash_window::create_surface(
                &gfx_core.vk_entry,
                &gfx_core.instance.ash_instance,
                raw_display_handle,
                raw_window_handle,
                None,
            )
            .unwrap()
        };

        let surface = GfxSurface {
            handle: surface,
            pf: surface_pf,
        };
        gfx_core.gfx_device.set_debug_name(&surface, "main");

        surface
    }
}

// getters
impl GfxSurface {
    /// 实时获取 surface 的能力信息
    pub fn get_capabilities(&self) -> vk::SurfaceCapabilitiesKHR {
        unsafe {
            self.pf
                .get_physical_device_surface_capabilities(Gfx::get().gfx_core.physical_device.vk_handle, self.handle)
                .unwrap()
        }
    }
}

impl Drop for GfxSurface {
    fn drop(&mut self) {
        unsafe { self.pf.destroy_surface(self.handle, None) }
    }
}

impl DebugType for GfxSurface {
    fn debug_type_name() -> &'static str {
        "GfxSurface"
    }
    fn vk_handle(&self) -> impl vk::Handle {
        self.handle
    }
}
