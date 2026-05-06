use std::{
    collections::HashSet,
    ffi::{CStr, CString, c_char},
    rc::Rc,
};

use ash::vk;
use itertools::Itertools;

use crate::foundation::debug_messenger::GfxDebugMsger;

pub struct GfxInstance {
    /// 仅仅是函数指针，以及一个裸的 handle，可以随意 clone
    ///
    /// 不需要考虑生命周期的问题，生命周期现在是由手动控制的
    pub(crate) ash_instance: Rc<ash::Instance>,
}

impl GfxInstance {
    /// 设置所需的 layers 和 extensions，创建 vk instance
    pub fn new(
        vk_entry: &ash::Entry,
        app_name: String,
        engine_name: String,
        extra_instance_exts: Vec<&'static CStr>,
    ) -> Self {
        let _span = tracy_client::span!("GfxInstance::new");

        let app_name = CString::new(app_name.as_str()).unwrap();
        let engine_name = CString::new(engine_name.as_str()).unwrap();
        let app_info = vk::ApplicationInfo::default()
            .api_version(vk::API_VERSION_1_3) // 版本过低时，有些函数无法正确加载
            .application_name(app_name.as_ref())
            .application_version(vk::make_api_version(0, 1, 0, 0))
            .engine_name(engine_name.as_ref())
            .engine_version(vk::make_api_version(0, 1, 0, 0));

        let enabled_extensions = Self::get_extensions(vk_entry, &extra_instance_exts);
        // 多行输出到一个字符串
        let mut enabled_extensions_str = String::new();
        for ext in &enabled_extensions {
            enabled_extensions_str.push_str(&format!("\n\t{:?}", unsafe { CStr::from_ptr(*ext) }));
        }
        log::info!("instance extensions: {}", enabled_extensions_str);

        let enabled_layers = Self::get_layers(vk_entry);
        let mut enabled_layers_str = String::new();
        for layer in &enabled_layers {
            enabled_layers_str.push_str(&format!("\n\t{:?}", unsafe { CStr::from_ptr(*layer) }));
        }
        log::info!("instance layers: {}", enabled_layers_str);

        let mut instance_ci = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(&enabled_extensions)
            .enabled_layer_names(&enabled_layers);

        // 为 instance info 添加 debug messenger
        let mut debug_utils_messenger_ci = GfxDebugMsger::debug_utils_messenger_ci();
        instance_ci = instance_ci.push_next(&mut debug_utils_messenger_ci);

        let handle = unsafe { vk_entry.create_instance(&instance_ci, None).unwrap() };

        Self {
            ash_instance: Rc::new(handle),
        }
    }

    pub fn destroy(self) {
        log::info!("Destroying GfxInstance");
        unsafe {
            self.ash_instance.destroy_instance(None);
        }
    }
}

// 访问器
impl GfxInstance {
    #[inline]
    pub fn ash_instance(&self) -> &ash::Instance {
        &self.ash_instance
    }

    #[inline]
    pub fn vk_instance(&self) -> vk::Instance {
        self.ash_instance.handle()
    }
}

// 构造过程
impl GfxInstance {
    /// 用于在创建 instance 时设置 layer 的参数
    fn _get_layer_setting_for_single<'a, T>(
        layer_name: &'static CStr,
        setting_name: &'static CStr,
        ty: vk::LayerSettingTypeEXT,
        value: &'a T,
    ) -> vk::LayerSettingEXT<'a> {
        vk::LayerSettingEXT {
            p_layer_name: layer_name.as_ptr(),
            p_setting_name: setting_name.as_ptr(),
            ty,
            value_count: 1,
            p_values: value as *const _ as *const std::ffi::c_void,
            ..Default::default()
        }
    }

    /// 用于在创建 instance 时设置 layer 的参数
    fn _get_layer_setting_for_array<'a, T>(
        layer_name: &'static CStr,
        setting_name: &'static CStr,
        ty: vk::LayerSettingTypeEXT,
        value: &'a [T],
    ) -> vk::LayerSettingEXT<'a> {
        vk::LayerSettingEXT {
            p_layer_name: layer_name.as_ptr(),
            p_setting_name: setting_name.as_ptr(),
            ty,
            value_count: value.len() as u32,
            p_values: value.as_ptr() as *const std::ffi::c_void,
            ..Default::default()
        }
    }

    /// instance 所需的所有 extension
    ///
    /// # 返回
    /// instance 所需的，且受支持的 extension
    fn get_extensions(vk_entry: &ash::Entry, extra_instance_exts: &[&'static CStr]) -> Vec<*const c_char> {
        let all_ext_props = unsafe { vk_entry.enumerate_instance_extension_properties(None).unwrap() };
        let mut enabled_extensions: HashSet<&'static CStr> = HashSet::new();

        // 检查某个 instance ext 并启用
        let mut enable_ext = |ext: &'static CStr| {
            let supported = all_ext_props
                .iter()
                .any(|supported_ext| ext == unsafe { CStr::from_ptr(supported_ext.extension_name.as_ptr()) });
            if supported {
                enabled_extensions.insert(ext);
            } else {
                panic!("Required instance extensions ({:?}) are missin", ext)
            }
        };

        // 检查外部传入的 extension 是否支持
        for ext in extra_instance_exts {
            enable_ext(ext);
        }

        for ext in Self::basic_instance_exts() {
            enable_ext(ext);
        }

        enabled_extensions.iter().map(|ext| ext.as_ptr()).collect_vec()
    }

    /// instance 所需的所有 layers
    fn get_layers(vk_entry: &ash::Entry) -> Vec<*const c_char> {
        let all_layer_props = unsafe { vk_entry.enumerate_instance_layer_properties().unwrap() };

        // 存储所有支持的 layers
        let mut valid_layers = Vec::new();

        // 检查并启用某个 instance layer
        let mut enable_layer = |layer: &'static CStr| {
            let is_layer_supported = all_layer_props
                .iter()
                .any(|available_layer| layer == unsafe { CStr::from_ptr(available_layer.layer_name.as_ptr()) });
            if is_layer_supported {
                valid_layers.push(layer);
            } else {
                panic!("Required instance layers ({:?}) are missing", layer);
            }
        };

        for layer in Self::basic_instance_layers() {
            enable_layer(layer);
        }

        valid_layers.iter().map(|ext| ext.as_ptr()).collect_vec()
    }

    /// 必须要开启的 instance layers
    fn basic_instance_layers() -> Vec<&'static CStr> {
        // 无需开启 validation layer，使用 vulkan configurator 控制 validation layer
        // 的开启 layers.push(cstr::cstr!("VK_LAYER_KHRONOS_validation"))

        Vec::new()
    }

    /// 必须要开启的 instance extensions
    fn basic_instance_exts() -> Vec<&'static CStr> {
        let exts = vec![
            // 这个 extension 可以单独使用，提供以下功能：
            // 1. debug messenger
            // 2. 为 vulkan object 设置 debug name
            // 2. 使用 label 标记 queue 或者 command buffer 中的一个一个 section
            // 这个 extension 可以和 validation layer 配合使用，提供更详细的信息
            vk::EXT_DEBUG_UTILS_NAME,
        ];

        exts
    }
}
