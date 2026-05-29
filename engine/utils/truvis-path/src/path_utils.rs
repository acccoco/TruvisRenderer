use std::{
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};

/// 和进程运行位置、Windows 路径编码相关的通用工具。
///
/// `TruvisPath` 负责 workspace 内固定目录；`PathUtils` 负责运行时才能知道的路径，
/// 例如当前 executable 位置，以及传给 Windows / C++ wide API 的 UTF-16 路径。
pub struct PathUtils;

impl PathUtils {
    /// 当前进程 executable 的完整路径。
    ///
    /// 该值由操作系统返回，通常位于 Cargo 输出目录，如当前配置下的
    /// `build/{profile}` 或 `build/{profile}/examples`。
    pub fn current_exe_path() -> std::io::Result<PathBuf> {
        std::env::current_exe()
    }

    /// 当前进程 executable 所在目录。
    ///
    /// Native runtime DLL 通常需要与 executable 位于同一目录；Streamline 的
    /// `sl.interposer.dll`、`sl.dlss.dll`、`nvngx_dlss.dll`、`NvLowLatencyVk.dll`
    /// 就依赖这个约定。
    pub fn current_exe_dir() -> std::io::Result<PathBuf> {
        let exe = Self::current_exe_path()?;
        Ok(exe.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")))
    }

    /// 将 Rust path 转成 null-terminated UTF-16 buffer。
    ///
    /// Windows C++ API 和 Streamline 的路径字段使用 `wchar_t*`，在 Windows 上等价于
    /// UTF-16。返回值包含末尾 `0`，可以在 FFI 调用期间直接作为 `*const u16` 传递。
    pub fn path_to_utf16_null_terminated(path: impl AsRef<Path>) -> Vec<u16> {
        path.as_ref().as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }
}
