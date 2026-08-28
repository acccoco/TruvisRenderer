#pragma once

#include <string>

namespace truvixx::utils
{

/// Windows 路径与目录工具。
///
/// 该 struct 只聚合无状态 helper，不需要实例化。它和 Rust 侧的 `PathUtils`
/// 承担类似职责：收拢 executable 目录、运行期默认目录和文件系统检查等通用逻辑。
struct PathUtils
{
    /// 当前进程 executable 所在目录。
    ///
    /// Native runtime DLL 应与 executable 同目录。Streamline 路径用它作为默认 plugin
    /// 搜索目录，确保 ash 和 C++ wrapper 命中同一份 `sl.interposer.dll`。
    static std::wstring current_executable_dir();

    /// Streamline 默认日志目录。
    ///
    /// 该函数只提供项目当前约定：从进程工作目录写入 `.temp/streamline`。
    /// 如果应用改变 working directory，应由调用方显式传入日志目录。
    static std::wstring default_temp_streamline_log_dir();

    /// 确保目录存在。
    ///
    /// `path` 为空时视为无需创建并返回 true。失败时返回 false，并把可打印的 UTF-8
    /// 错误文本写入 `error_message`。
    static bool ensure_directory(const wchar_t* path, std::string* error_message);
};

} // namespace truvixx::utils
