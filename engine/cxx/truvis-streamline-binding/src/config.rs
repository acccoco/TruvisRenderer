//! Streamline 初始化配置与运行时路径约定。
//!
//! 本模块只负责 Rust 侧可配置输入和默认路径推导，不直接调用 SL API。
//! 进程级 init/shutdown 生命周期由 `runtime` 模块维护。

use std::{env, path::PathBuf};

use truvis_path::{PathUtils, TruvisPath};

use crate::truvixx;

const STREAMLINE_IMGUI_ENV: &str = "TRUVIS_STREAMLINE_IMGUI";

/// Streamline feature 请求位。
///
/// Rust 侧决定要加载哪些 feature，C++ wrapper 只负责把这些稳定 bit 翻译成
/// Streamline SDK 的 feature id。默认只加载 DLSS SR；Debug 可通过环境变量显式打开 SL ImGui。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamlineFeatureFlags(u32);

impl StreamlineFeatureFlags {
    pub const DLSS: Self = Self(truvixx::TruvixxSlFeatureFlag_TruvixxSlFeatureFlagDlss);
    pub const IMGUI: Self = Self(truvixx::TruvixxSlFeatureFlag_TruvixxSlFeatureFlagImgui);

    #[inline]
    pub const fn bits(self) -> u32 {
        self.0
    }

    #[inline]
    pub const fn contains(self, feature: Self) -> bool {
        (self.0 & feature.0) == feature.0
    }

    #[inline]
    pub fn insert(&mut self, feature: Self) {
        self.0 |= feature.0;
    }

    pub fn display_names(self) -> String {
        let mut names = Vec::new();
        if self.contains(Self::DLSS) {
            names.push("DLSS");
        }
        if self.contains(Self::IMGUI) {
            names.push("ImGui");
        }

        match names.as_slice() {
            [] => "<none>".to_string(),
            _ => names.join("|"),
        }
    }
}

impl Default for StreamlineFeatureFlags {
    fn default() -> Self {
        let mut flags = Self::DLSS;
        if should_enable_streamline_imgui() {
            flags.insert(Self::IMGUI);
        }
        flags
    }
}

fn should_enable_streamline_imgui() -> bool {
    let env_value = match env::var(STREAMLINE_IMGUI_ENV) {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return false,
        Err(env::VarError::NotUnicode(value)) => {
            log::warn!("{} contains non-unicode value {:?}; SL ImGui disabled.", STREAMLINE_IMGUI_ENV, value);
            return false;
        }
    };

    match parse_bool_env(&env_value) {
        Some(false) => false,
        Some(true) if cfg!(debug_assertions) => true,
        Some(true) => {
            log::warn!(
                "{}={} requested SL ImGui, but release runtime does not copy sl.imgui.dll; SL ImGui disabled.",
                STREAMLINE_IMGUI_ENV,
                env_value
            );
            false
        }
        None => {
            log::warn!(
                "Invalid {} value `{}`; expected one of 1/true/on/yes/enable/enabled or 0/false/off/no/disable/disabled. SL ImGui disabled.",
                STREAMLINE_IMGUI_ENV,
                env_value
            );
            false
        }
    }
}

fn parse_bool_env(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" | "enable" | "enabled" => Some(true),
        "0" | "false" | "off" | "no" | "disable" | "disabled" => Some(false),
        _ => None,
    }
}

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

    /// 请求加载的 Streamline feature。默认只包含 DLSS SR；Debug 可通过
    /// `TRUVIS_STREAMLINE_IMGUI` 显式请求 SL ImGui。
    pub feature_flags: StreamlineFeatureFlags,
}

impl Default for StreamlineInitInfo {
    fn default() -> Self {
        Self {
            plugin_dir: PathUtils::current_exe_dir().unwrap_or_else(|_| PathBuf::from(".")),
            log_dir: TruvisPath::temp_dir().join("streamline"),
            show_console: false,
            verbose_log: true,
            feature_flags: StreamlineFeatureFlags::default(),
        }
    }
}

/// 返回当前 executable 目录中的 Streamline Vulkan loader 路径。
///
/// 这个 helper 只计算路径，不检查 DLL 是否存在；DLL 布置由 `cxx-build` 负责。
pub fn default_vulkan_loader_path() -> Result<PathBuf, std::io::Error> {
    Ok(PathUtils::current_exe_dir()?.join("sl.interposer.dll"))
}
