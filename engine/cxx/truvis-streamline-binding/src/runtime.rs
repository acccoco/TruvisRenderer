//! Streamline 进程级 runtime 生命周期。
//!
//! 本模块负责 `slInit` / `slShutdown` 的 RAII 封装、日志桥生命周期和 Vulkan
//! interposer loader 路径暴露。配置输入由 `config` 模块提供。

use std::{
    fmt,
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
};

use truvis_path::PathUtils;

use crate::{config::StreamlineInitInfo, log_bridge::StreamlineLogBridge, truvixx};

/// Streamline wrapper 错误。
///
/// `sl_result` 是 `sl::Result` 的原始整数值（0 = 成功）。
/// 具体错误细节通过日志桥的 error 级别消息获取。
#[derive(Debug, Clone)]
pub struct StreamlineError {
    sl_result: i32,
    context: &'static str,
}

impl StreamlineError {
    fn new(sl_result: i32, context: &'static str) -> Self {
        Self { sl_result, context }
    }

    /// `sl::Result` 的原始整数值，0 表示成功。
    #[inline]
    pub fn sl_result(&self) -> i32 {
        self.sl_result
    }
}

impl fmt::Display for StreamlineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Streamline {} failed (sl::Result={})", self.context, self.sl_result)
    }
}

impl std::error::Error for StreamlineError {}

/// 进程级 Streamline runtime 句柄。
///
/// 该类型的存在即表示 Streamline runtime 已成功初始化。创建 = init，drop = shutdown。
/// `!Send + !Sync` 通过 `PhantomData<Rc<()>>` 保证，防止跨线程误用。
pub struct StreamlineRuntime {
    plugin_dir: PathBuf,
    log_dir: PathBuf,
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
    pub fn init(info: StreamlineInitInfo) -> Result<Self, StreamlineError> {
        log::info!(
            "Initializing Streamline runtime: plugin_dir={}, log_dir={}, show_console={}, verbose_log={}",
            info.plugin_dir.display(),
            info.log_dir.display(),
            info.show_console,
            info.verbose_log
        );

        // Rust 侧负责目录创建，C++ 不做路径校验。
        if let Err(err) = std::fs::create_dir_all(&info.log_dir) {
            log::error!("Failed to create Streamline log dir {}: {}", info.log_dir.display(), err);
            return Err(StreamlineError::new(-1, "log dir creation"));
        }

        // 日志桥必须在 slInit 之前就绑，因为 slInit 内部会同步触发 log callback。
        let log_bridge = StreamlineLogBridge::new().map_err(|err| {
            log::error!("Failed to start Streamline log drain thread: {}", err);
            StreamlineError::new(-1, "log bridge creation")
        })?;

        let plugin_dir_utf16 = PathUtils::path_to_utf16_null_terminated(&info.plugin_dir);
        let log_dir_utf16 = PathUtils::path_to_utf16_null_terminated(&info.log_dir);

        let desc = truvixx::TruvixxSlInitDesc {
            plugin_dir_utf16: plugin_dir_utf16.as_ptr(),
            log_dir_utf16: log_dir_utf16.as_ptr(),
            show_console: u32::from(info.show_console),
            verbose_log: u32::from(info.verbose_log),
            log_callback: StreamlineLogBridge::callback(),
        };

        let result = unsafe { truvixx::truvixx_sl_init(&desc) };
        if result != 0 {
            let err = StreamlineError::new(result, "slInit");
            log::error!("Streamline runtime initialization failed: {}", err);
            return Err(err);
        }

        let runtime = Self {
            plugin_dir: info.plugin_dir,
            log_dir: info.log_dir,
            _log_bridge: log_bridge,
            _not_send_sync: PhantomData,
        };
        log::info!(
            "Streamline runtime initialized: plugin_dir={}, vulkan_loader={}, log_dir={}",
            runtime.plugin_dir().display(),
            runtime.vulkan_loader_path().display(),
            runtime.log_dir().display()
        );

        Ok(runtime)
    }

    /// Streamline plugin/runtime 目录。
    #[inline]
    pub fn plugin_dir(&self) -> &Path {
        &self.plugin_dir
    }

    /// Streamline 日志和诊断数据目录。
    #[inline]
    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }

    /// Vulkan loader DLL 路径。
    ///
    /// DLSS 路径应把该路径传给 `truvis-gfx` 的 `VulkanEntrySource::DllPath`，让 ash
    /// 从 `sl.interposer.dll` 获取 `vkGetInstanceProcAddr` / `vkGetDeviceProcAddr`。
    #[inline]
    pub fn vulkan_loader_path(&self) -> PathBuf {
        self.plugin_dir.join("sl.interposer.dll")
    }
}

impl Drop for StreamlineRuntime {
    fn drop(&mut self) {
        let result = unsafe { truvixx::truvixx_sl_shutdown() };
        if result == 0 {
            log::info!("Streamline runtime shutdown completed.");
        } else {
            log::error!("Streamline runtime shutdown failed (sl::Result={})", result);
        }
        // _log_bridge drop 在这之后发生（struct 字段按声明顺序 drop），
        // 保证 shutdown 期间的最后几条日志仍然能被 drain 线程处理。
    }
}
