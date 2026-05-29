//! Streamline 初始化配置与运行时路径约定。
//!
//! 本模块只负责 Rust 侧可配置输入和默认路径推导，不直接调用 SL API。
//! 进程级 init/shutdown 生命周期由 `runtime` 模块维护。

use std::path::PathBuf;

use truvis_path::{PathUtils, TruvisPath};

/// Streamline 初始化参数。
///
/// `plugin_dir` 是 Streamline plugin/runtime 搜索目录，必须包含 `sl.interposer.dll`、
/// `sl.common.dll`、`sl.pcl.dll`、`sl.dlss.dll` 和 `nvngx_dlss.dll`。项目的
/// `cxx-build` 会把这些文件复制到 `build/{profile}` 和
/// `build/{profile}/examples`，所以默认值使用当前 executable 所在目录。
///
/// `log_dir` 是 Streamline 日志目录。默认放在 `.temp/streamline`，这样运行目录只承担
/// DLL/JSON 布置职责，诊断日志不会混在 `build` 产物中。
#[derive(Clone, Debug)]
pub struct StreamlineInitInfo {
    /// Streamline runtime/plugin 目录，通常是当前 executable 所在目录。
    pub plugin_dir: PathBuf,

    /// Streamline 日志和诊断数据目录，初始化前会由 Rust 侧主动创建。
    pub log_dir: PathBuf,

    /// 是否让 Streamline 打开调试控制台。
    pub show_console: bool,

    /// 是否使用 verbose 级别日志。
    pub verbose_log: bool,
}

impl Default for StreamlineInitInfo {
    fn default() -> Self {
        Self {
            plugin_dir: PathUtils::current_exe_dir().unwrap_or_else(|_| PathBuf::from(".")),
            log_dir: TruvisPath::temp_dir().join("streamline"),
            show_console: false,
            verbose_log: true,
        }
    }
}

/// 返回当前 executable 目录中的 Streamline Vulkan loader 路径。
///
/// 这个 helper 只计算路径，不检查 DLL 是否存在；DLL 布置由 `cxx-build` 负责。
pub fn default_vulkan_loader_path() -> Result<PathBuf, std::io::Error> {
    Ok(PathUtils::current_exe_dir()?.join("sl.interposer.dll"))
}
