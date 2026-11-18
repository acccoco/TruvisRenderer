use std::ffi::CStr;

use ash::vk;

pub struct GfxDebugMsger {
    pub vk_debug_utils_instance: ash::ext::debug_utils::Instance,
    pub vk_debug_utils_messenger: vk::DebugUtilsMessengerEXT,
}

impl GfxDebugMsger {
    pub fn new(vk_pf: &ash::Entry, instance: &ash::Instance) -> Self {
        let loader = ash::ext::debug_utils::Instance::new(vk_pf, instance);

        let create_info = Self::debug_utils_messenger_ci();
        let debug_messenger = unsafe { loader.create_debug_utils_messenger(&create_info, None).unwrap() };

        Self {
            vk_debug_utils_instance: loader,
            vk_debug_utils_messenger: debug_messenger,
        }
    }

    /// 显式释放 debug messenger，必须在 Instance 销毁前调用。
    pub fn destroy(mut self) {
        if self.vk_debug_utils_messenger == vk::DebugUtilsMessengerEXT::null() {
            return;
        }
        unsafe {
            self.vk_debug_utils_instance.destroy_debug_utils_messenger(self.vk_debug_utils_messenger, None);
        }
        self.vk_debug_utils_messenger = vk::DebugUtilsMessengerEXT::null();
    }
}

impl Drop for GfxDebugMsger {
    fn drop(&mut self) {
        debug_assert!(
            self.vk_debug_utils_messenger == vk::DebugUtilsMessengerEXT::null(),
            "GfxDebugMsger dropped without explicit destroy"
        );
    }
}

/// debug messenger 的回调函数；诊断失败不能让 Rust unwind 越过 Vulkan ABI。
/// logger/panic hook 不得重入 Vulkan；这里只隔离可捕获的 unwind，不改变同步日志行为。
///
/// # Safety
/// 非空 callback data 及其非空 message 指针必须在本次调用中有效，message 必须以 NUL 结尾。
unsafe extern "system" fn vk_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _user_data: *mut std::os::raw::c_void,
) -> vk::Bool32 {
    if p_callback_data.is_null() {
        return vk::FALSE;
    }

    let outcome = std::panic::catch_unwind(|| {
        let callback_data = unsafe { *p_callback_data };

        let msg = if callback_data.p_message.is_null() {
            std::borrow::Cow::from("")
        } else {
            unsafe { CStr::from_ptr(callback_data.p_message).to_string_lossy() }
        };

        // 提取 json 里面的 MainMessage 字段，这个字段里面有换行符，需要单独输出
        let mut json_value = serde_json::from_str::<serde_json::Value>(msg.as_ref());
        let mut json_obj = json_value.as_mut().map_or(None, |v| v.as_object_mut());
        let mut main_msg_value = None;
        if let Some(obj) = &mut json_obj {
            main_msg_value = obj.remove("MainMessage");
        }
        let main_msg_str = main_msg_value.as_ref().and_then(|value| value.as_str()).unwrap_or_default();
        let total_msg_str =
            json_obj.and_then(|obj| serde_json::to_string_pretty(&obj).ok()).unwrap_or_else(|| msg.to_string());

        let format_msg = format!("[{:?}]\n{}\n{}\n", message_type, total_msg_str, main_msg_str);

        match message_severity {
            vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => {
                log::error!("{}", format_msg);
            }
            vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => {
                log::warn!("{}", format_msg);
            }
            vk::DebugUtilsMessageSeverityFlagsEXT::INFO => {
                log::info!("{}", format_msg);
            }
            _ => log::info!("{}", format_msg),
        };
    });
    if let Err(payload) = outcome {
        // panic payload 的 Drop 也可能 panic；失败路径不再日志输出或执行该析构。
        std::mem::forget(payload);
    }

    // 诊断从不请求 Vulkan 调用中止；日志后端失败也保持这一返回约定。
    vk::FALSE
}

// 构造过程辅助函数
impl GfxDebugMsger {
    /// 固定的诊断配置，无共享可变状态；可由任意线程并发构造 messenger 描述。
    pub fn debug_msg_type() -> vk::DebugUtilsMessageTypeFlagsEXT {
        vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
            | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
            | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
    }

    /// 固定的诊断配置，无共享可变状态；可由任意线程并发构造 messenger 描述。
    pub fn debug_msg_severity() -> vk::DebugUtilsMessageSeverityFlagsEXT {
        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
    }

    /// 用于创建 debug messenger 的结构体
    pub fn debug_utils_messenger_ci() -> vk::DebugUtilsMessengerCreateInfoEXT<'static> {
        vk::DebugUtilsMessengerCreateInfoEXT::default()
            .message_severity(Self::debug_msg_severity())
            .message_type(Self::debug_msg_type())
            .pfn_user_callback(Some(vk_debug_callback))
    }
}

pub trait DebugType {
    fn debug_type_name() -> &'static str;
    fn vk_handle(&self) -> impl vk::Handle;
}
