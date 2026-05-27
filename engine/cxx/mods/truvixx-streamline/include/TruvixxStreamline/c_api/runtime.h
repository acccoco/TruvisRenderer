#pragma once

#include "TruvixxStreamline/c_api/truvixx_streamline.export.h"

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Streamline C API 调用结果。
///
/// 该枚举只表达 truvixx wrapper 的稳定状态；具体 SL 错误码通过
/// `truvixx_sl_last_error_utf8` 暴露，避免 Rust 侧直接依赖 Streamline C++ ABI。
/// 后续如果需要细分 DLSS feature 不可用、driver 过旧等情况，应继续新增项目自己的
/// C ABI 错误码，而不是把 `sl::Result` 直接暴露给 Rust。
typedef enum : uint32_t
{
    TruvixxSlResultOk = 0,
    TruvixxSlResultAlreadyInitialized = 1,
    TruvixxSlResultNotInitialized = 2,
    TruvixxSlResultInvalidArgument = 3,
    TruvixxSlResultStreamlineError = 4,
} TruvixxSlResult;

/// Streamline 日志类型。
///
/// 该枚举隔离 Streamline C++ ABI，Rust 侧只依赖项目自己的稳定日志级别。
typedef enum : uint32_t
{
    TruvixxSlLogTypeInfo = 0,
    TruvixxSlLogTypeWarn = 1,
    TruvixxSlLogTypeError = 2,
} TruvixxSlLogType;

/// Streamline 日志回调。
///
/// 该回调可能在 `slInit`、`slShutdown` 或 Vulkan interposer 调用栈中触发。实现方必须把它
/// 当作任意线程、可重入、不可阻塞的 FFI callback 处理；`message_utf8` 只在本次调用期间有效。
typedef void (*TruvixxSlLogCallback)(
    TruvixxSlLogType type,
    const char* message_utf8,
    uint32_t message_len,
    uint32_t native_thread_id,
    void* user_data
);

/// Streamline 初始化描述。
///
/// 路径字段使用 null-terminated UTF-16，调用方可传空指针表示使用 wrapper 默认值。
/// 当前阶段只加载 DLSS Super Resolution，不启用 manual hooking 或 Frame Generation。
///
/// `plugin_dir_utf16` 指向 Streamline runtime/plugin 所在目录。项目约定该目录是
/// executable 目录，由 `cxx-build` 从 `tools/streamline-sdk` 按 profile 复制 DLL/JSON。
/// `log_dir_utf16` 指向 Streamline 日志目录，用于定位 plugin 缺失、driver 不支持等问题。
///
/// `log_callback` 由 Rust 侧传入，用于把 Streamline 日志事件接入项目统一日志系统。
typedef struct
{
    const uint16_t* plugin_dir_utf16;
    const uint16_t* log_dir_utf16;
    uint32_t show_console;
    uint32_t verbose_log;
    TruvixxSlLogCallback log_callback;
    void* log_user_data;
} TruvixxSlInitDesc;

/// 初始化 Streamline runtime，并只加载 DLSS SR feature。
TruvixxSlResult TRUVIXX_STREAMLINE_API truvixx_sl_init(const TruvixxSlInitDesc* desc);

/// 关闭 Streamline runtime。
TruvixxSlResult TRUVIXX_STREAMLINE_API truvixx_sl_shutdown();

/// 查询当前进程内 Streamline wrapper 是否处于已初始化状态。
uint32_t TRUVIXX_STREAMLINE_API truvixx_sl_is_initialized();

/// 返回最近一次 wrapper 或 Streamline 错误，字符串所有权归 C++ wrapper。
TRUVIXX_STREAMLINE_API const char* truvixx_sl_last_error_utf8();

#ifdef __cplusplus
}
#endif
