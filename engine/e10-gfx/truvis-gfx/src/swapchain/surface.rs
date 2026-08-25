use ash::vk;
use ash::vk::Handle;

use crate::foundation::debug_messenger::DebugType;
use crate::gfx::GfxSurfaceCtx;

pub struct GfxSurface {
    pub(crate) handle: vk::SurfaceKHR,
    pub(crate) pf: ash::khr::surface::Instance,
}

impl GfxSurface {
    pub fn new(
        ctx: GfxSurfaceCtx<'_>,
        raw_display_handle: raw_window_handle::RawDisplayHandle,
        raw_window_handle: raw_window_handle::RawWindowHandle,
    ) -> Self {
        let gfx_core = ctx.core();
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

// 访问器
impl GfxSurface {
    /// 实时获取 surface 的能力信息
    pub fn get_capabilities(&self, ctx: GfxSurfaceCtx<'_>) -> vk::SurfaceCapabilitiesKHR {
        unsafe {
            self.pf.get_physical_device_surface_capabilities(ctx.physical_device().vk_handle, self.handle).unwrap()
        }
    }

    /// 查询当前 surface 支持的颜色格式。
    pub fn supported_formats(&self, ctx: GfxSurfaceCtx<'_>) -> Vec<vk::SurfaceFormatKHR> {
        unsafe { self.pf.get_physical_device_surface_formats(ctx.physical_device().vk_handle, self.handle).unwrap() }
    }

    /// 查询当前 surface 支持的 present mode。
    pub fn supported_present_modes(&self, ctx: GfxSurfaceCtx<'_>) -> Vec<vk::PresentModeKHR> {
        unsafe {
            self.pf.get_physical_device_surface_present_modes(ctx.physical_device().vk_handle, self.handle).unwrap()
        }
    }

    pub fn destroy(mut self, _ctx: GfxSurfaceCtx<'_>) {
        if self.handle.is_null() {
            return;
        }
        unsafe {
            self.pf.destroy_surface(self.handle, None);
        }
        self.handle = vk::SurfaceKHR::null();
    }
}

impl Drop for GfxSurface {
    fn drop(&mut self) {
        debug_assert!(self.handle.is_null(), "GfxSurface dropped without explicit destroy");
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
