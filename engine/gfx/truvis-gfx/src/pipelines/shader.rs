use std::collections::HashMap;
use std::ffi::CStr;

use ash::vk;
use ash::vk::Handle;

use crate::{foundation::debug_messenger::DebugType, gfx::GfxDeviceCtx};

/// # 销毁
///
/// 需要手动调用 `destroy` 方法来释放资源。
pub struct GfxShaderModule {
    handle: vk::ShaderModule,
    debug_name: String,

    #[cfg(debug_assertions)]
    destroyed: bool,
}
impl GfxShaderModule {
    /// # 参数
    /// * path - spv shader 文件路径
    pub fn new(ctx: GfxDeviceCtx<'_>, path: &std::path::Path) -> Self {
        let gfx_device = ctx.device();
        let mut file = std::fs::File::open(path).unwrap();
        let shader_code = ash::util::read_spv(&mut file).unwrap();

        let shader_module_info = vk::ShaderModuleCreateInfo::default().code(&shader_code);

        unsafe {
            let shader_module = gfx_device.create_shader_module(&shader_module_info, None).unwrap();
            let shader_module = Self {
                handle: shader_module,
                debug_name: path.to_string_lossy().to_string(),

                #[cfg(debug_assertions)]
                destroyed: false,
            };
            gfx_device.set_debug_name(&shader_module, path.to_str().unwrap());
            shader_module
        }
    }

    #[inline]
    pub fn handle(&self) -> vk::ShaderModule {
        self.handle
    }

    #[inline]
    pub fn destroy(mut self, ctx: GfxDeviceCtx<'_>) {
        if self.handle.is_null() {
            return;
        }
        let gfx_device = ctx.device();
        unsafe {
            gfx_device.destroy_shader_module(self.handle, None);
        }
        self.handle = vk::ShaderModule::null();
        #[cfg(debug_assertions)]
        {
            self.destroyed = true;
        }
    }
}
impl Drop for GfxShaderModule {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        debug_assert!(
            self.destroyed && self.handle.is_null(),
            "GfxShaderModule '{}' dropped without explicit destroy",
            self.debug_name
        );
    }
}
impl DebugType for GfxShaderModule {
    fn debug_type_name() -> &'static str {
        "GfxShaderModule"
    }

    fn vk_handle(&self) -> impl vk::Handle {
        self.handle
    }
}

/// 可以存放多个 ShaderModule，使用路径进行索引
pub struct GfxShaderModuleCache {
    shader_modules: HashMap<String, GfxShaderModule>,
    #[cfg(debug_assertions)]
    destroyed: bool,
}
impl Default for GfxShaderModuleCache {
    fn default() -> Self {
        Self::new()
    }
}

impl GfxShaderModuleCache {
    pub fn new() -> Self {
        Self {
            shader_modules: HashMap::new(),
            #[cfg(debug_assertions)]
            destroyed: false,
        }
    }

    pub fn get_or_load(&mut self, ctx: GfxDeviceCtx<'_>, path: &std::path::Path) -> &GfxShaderModule {
        let path_str = path.to_str().unwrap().to_string();
        self.shader_modules.entry(path_str).or_insert_with(|| GfxShaderModule::new(ctx, path))
    }

    pub fn destroy(mut self, ctx: GfxDeviceCtx<'_>) {
        #[cfg(debug_assertions)]
        {
            self.destroyed = true;
        }

        // 使用 std::mem::take 来 move 出 HashMap，留下一个空的 HashMap
        let shader_modules = std::mem::take(&mut self.shader_modules);
        shader_modules.into_values().for_each(|module| module.destroy(ctx));
    }
}
impl Drop for GfxShaderModuleCache {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        debug_assert!(self.destroyed, "ShaderModuleCache must be destroyed manually before drop.");
    }
}

#[derive(Clone)]
pub struct GfxShaderStageInfo {
    pub stage: vk::ShaderStageFlags,
    pub entry_point: &'static CStr,
    pub path: String,
}
impl GfxShaderStageInfo {
    #[inline]
    pub fn path(&self) -> &std::path::Path {
        std::path::Path::new(self.path.as_str())
    }
}

/// 用于 RayTracing Pipeline 的创建
///
/// 在 pipeline create info 的 groups 中，每个 shader group 的索引
///
/// 每个 shader group 可以由多个 shader 组成，每个 shader group 都是独一无二的
pub struct GfxShaderGroupInfo {
    pub ty: vk::RayTracingShaderGroupTypeKHR,
    pub general: u32,
    pub closest_hit: u32,
    pub any_hit: u32,
    pub intersection: u32,
}
impl GfxShaderGroupInfo {
    pub const fn unused() -> Self {
        Self {
            ty: vk::RayTracingShaderGroupTypeKHR::GENERAL,
            general: vk::SHADER_UNUSED_KHR,
            closest_hit: vk::SHADER_UNUSED_KHR,
            any_hit: vk::SHADER_UNUSED_KHR,
            intersection: vk::SHADER_UNUSED_KHR,
        }
    }
}
