//! Streamline / DLSS C++ 互操作层。
//!
//! 本 crate 只面向 Windows x64。它不做跨平台抽象，也不隐藏 Windows 路径编码细节：
//! Streamline SDK 的接口需要 UTF-16 路径，Vulkan loader 也会以 Windows DLL 的形式
//! 从 executable 所在目录加载。
//!
//! 当前阶段只覆盖 Streamline runtime 生命周期：
//! - C++ 侧负责调用 `slInit` / `slShutdown`，并隐藏 Streamline C++ ABI。
//! - Rust 侧只持有一个进程级 RAII 句柄，保证 drop 时触发 shutdown。
//! - Vulkan object、RenderGraph pass、resource tagging、`slEvaluateFeature` 都属于后续阶段。
//!
//! 重要生命周期约定：启用 DLSS 时，应先初始化 [`StreamlineRuntime`]，再用
//! [`StreamlineRuntime::vulkan_loader_path`] 创建 `truvis-gfx` 的 Streamline Vulkan loader。
//! 关闭时顺序相反，所有 Vulkan 对象必须先销毁，最后 drop [`StreamlineRuntime`]。

use std::{
    ffi::CStr,
    fmt,
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
};

use truvis_path::{PathUtils, TruvisPath};

pub mod _ffi_bindings;
pub use crate::_ffi_bindings::root as truvixx;

mod log_bridge;

use log_bridge::StreamlineLogBridge;

/// Streamline 初始化参数。
///
/// `plugin_dir` 是 Streamline plugin/runtime 搜索目录，必须包含 `sl.interposer.dll`、
/// `sl.common.dll`、`sl.pcl.dll`、`sl.dlss.dll` 和 `nvngx_dlss.dll`。项目的
/// `cxx-build` 会把这些文件复制到 `target/{profile}` 和
/// `target/{profile}/examples`，所以默认值使用当前 executable 所在目录。
///
/// `log_dir` 是 Streamline 日志目录。默认放在 `.temp/streamline`，这样运行目录只承担
/// DLL/JSON 布置职责，诊断日志不会混在 `target` 产物中。
#[derive(Clone, Debug)]
pub struct StreamlineInitInfo {
    /// Streamline runtime/plugin 目录，通常是当前 executable 所在目录。
    pub plugin_dir: PathBuf,

    /// Streamline 日志和诊断数据目录，初始化前会由 Rust 侧主动创建。
    pub log_dir: PathBuf,

    /// 是否让 Streamline 打开调试控制台。Debug 默认开启，Release 默认关闭。
    pub show_console: bool,

    /// 是否使用 verbose 级别日志。Debug 默认开启，Release 默认关闭。
    pub verbose_log: bool,
}

impl Default for StreamlineInitInfo {
    fn default() -> Self {
        Self {
            plugin_dir: PathUtils::current_exe_dir().unwrap_or_else(|_| PathBuf::from(".")),
            log_dir: TruvisPath::temp_dir().join("streamline"),
            show_console: cfg!(debug_assertions),
            verbose_log: cfg!(debug_assertions),
        }
    }
}

/// Streamline wrapper 错误。
///
/// `result` 是项目自定义的稳定 C ABI 错误码；`message` 来自 C++ wrapper 保存的
/// 最近错误文本。Rust 侧不直接暴露 `sl::Result`，避免调用方绑定到 Streamline C++ ABI。
#[derive(Debug, Clone)]
pub struct StreamlineError {
    result: Option<truvixx::TruvixxSlResult>,
    message: String,
}

impl StreamlineError {
    fn native(result: truvixx::TruvixxSlResult) -> Self {
        Self {
            result: Some(result),
            message: last_error_message(),
        }
    }

    fn io(message: impl Into<String>, err: std::io::Error) -> Self {
        Self {
            result: None,
            message: format!("{}: {}", message.into(), err),
        }
    }

    /// C++ wrapper 返回的稳定错误码；Rust 自己产生的 IO 错误没有该值。
    #[inline]
    pub fn result(&self) -> Option<truvixx::TruvixxSlResult> {
        self.result
    }

    /// 可直接打印给日志的错误信息。
    #[inline]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for StreamlineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.result {
            Some(result) => write!(f, "Streamline error({result}): {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for StreamlineError {}

/// 进程级 Streamline runtime 句柄。
///
/// Streamline 的 `slInit` / `slShutdown` 是进程级生命周期 API，不是每个 viewport 或
/// 每个 Vulkan device 一个实例。本类型因此只表示“当前进程已经初始化 SL runtime”。
///
/// `_not_send_sync` 使用 `Rc` 标记该对象不能跨线程移动或共享。这样可以避免在未来引入
/// render thread / app thread 分离时，误把 Streamline 生命周期拆到多个线程里执行。
pub struct StreamlineRuntime {
    plugin_dir: PathBuf,
    _log_bridge: StreamlineLogBridge,
    _not_send_sync: PhantomData<Rc<()>>,
}

impl StreamlineRuntime {
    /// 使用项目默认路径初始化 Streamline。
    ///
    /// 默认假设 `cxx-build` 已把 Streamline runtime 复制到当前 executable 所在目录。
    pub fn init_default() -> Result<Self, StreamlineError> {
        Self::init(StreamlineInitInfo::default())
    }

    /// 初始化 Streamline，并只加载 DLSS Super Resolution feature。
    ///
    /// 这里把 Rust 的 `PathBuf` 转成 null-terminated UTF-16 临时 buffer。C++ wrapper
    /// 会在调用 `slInit` 期间同步消费这些指针；函数返回后临时 buffer 即可释放。
    pub fn init(info: StreamlineInitInfo) -> Result<Self, StreamlineError> {
        std::fs::create_dir_all(&info.log_dir)
            .map_err(|err| StreamlineError::io("failed to create Streamline log dir", err))?;

        let log_bridge = StreamlineLogBridge::new()
            .map_err(|err| StreamlineError::io("failed to start Streamline log drain thread", err))?;
        let plugin_dir_utf16 = PathUtils::path_to_utf16_null_terminated(&info.plugin_dir);
        let log_dir_utf16 = PathUtils::path_to_utf16_null_terminated(&info.log_dir);
        let desc = truvixx::TruvixxSlInitDesc {
            plugin_dir_utf16: plugin_dir_utf16.as_ptr(),
            log_dir_utf16: log_dir_utf16.as_ptr(),
            show_console: u32::from(info.show_console),
            verbose_log: u32::from(info.verbose_log),
            log_callback: StreamlineLogBridge::callback(),
            log_user_data: log_bridge.user_data(),
        };

        let result = unsafe { truvixx::truvixx_sl_init(&desc) };
        if result != truvixx::TruvixxSlResult_TruvixxSlResultOk {
            return Err(StreamlineError::native(result));
        }

        Ok(Self {
            plugin_dir: info.plugin_dir,
            _log_bridge: log_bridge,
            _not_send_sync: PhantomData,
        })
    }

    /// Streamline plugin/runtime 目录。
    #[inline]
    pub fn plugin_dir(&self) -> &Path {
        &self.plugin_dir
    }

    /// Vulkan loader DLL 路径。
    ///
    /// DLSS 路径应把该路径传给 `truvis-gfx` 的 `VulkanEntrySource::DllPath`，让 ash
    /// 从 `sl.interposer.dll` 获取 `vkGetInstanceProcAddr` / `vkGetDeviceProcAddr`。
    #[inline]
    pub fn vulkan_loader_path(&self) -> PathBuf {
        self.plugin_dir.join("sl.interposer.dll")
    }

    /// 查询 C++ wrapper 中记录的 Streamline 初始化状态。
    #[inline]
    pub fn is_initialized() -> bool {
        unsafe { truvixx::truvixx_sl_is_initialized() != 0 }
    }
}

impl Drop for StreamlineRuntime {
    fn drop(&mut self) {
        // Drop 不能返回错误；shutdown 失败只能留给 C++ wrapper 的 last_error 诊断。
        // 调用方仍应保证所有 Vulkan 对象已经先于该对象销毁。
        let _ = unsafe { truvixx::truvixx_sl_shutdown() };
    }
}

/// 返回当前 executable 目录中的 Streamline Vulkan loader 路径。
///
/// 这个 helper 只计算路径，不检查 DLL 是否存在；DLL 布置由 `cxx-build` 负责。
pub fn default_vulkan_loader_path() -> Result<PathBuf, StreamlineError> {
    Ok(PathUtils::current_exe_dir()
        .map_err(|err| StreamlineError::io("failed to get current executable dir", err))?
        .join("sl.interposer.dll"))
}

fn last_error_message() -> String {
    let ptr = unsafe { truvixx::truvixx_sl_last_error_utf8() };
    if ptr.is_null() {
        return "no Streamline error message".to_string();
    }

    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}
