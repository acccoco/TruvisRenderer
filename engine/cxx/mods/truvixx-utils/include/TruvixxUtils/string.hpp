#pragma once

#include <cstdint>
#include <string>
#include <string_view>

namespace truvixx::utils
{

/// Windows 字符串与 FFI 编码工具。
///
/// 该 struct 只聚合无状态 helper，不需要实例化。它的边界是编码和指针适配，
/// 不负责路径规范化或文件系统访问。
struct StringUtils
{
    /// 将 Windows UTF-16 字符串转换成 UTF-8。
    ///
    /// C++ wrapper 对 Rust 暴露的错误文本统一使用 UTF-8，便于 Rust 侧通过 `CStr`
    /// 读取并写入普通日志。该函数只做编码转换，不处理路径规范化。
    static std::string to_utf8(std::wstring_view value);

    /// 将 Rust FFI 传入的 UTF-16 指针转换成 Windows `wchar_t*`。
    ///
    /// `ptr` 为空时返回 `fallback.c_str()`；不为空时只做指针重解释，不复制数据、不接管所有权。
    /// 调用者必须保证 `ptr` 或 `fallback` 在下游同步调用期间有效。
    static const wchar_t* utf16_ptr_or_default(const uint16_t* ptr, const std::wstring& fallback);
};

} // namespace truvixx::utils
