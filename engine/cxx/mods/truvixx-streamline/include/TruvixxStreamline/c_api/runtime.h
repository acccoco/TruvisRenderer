#pragma once

#include "TruvixxStreamline/c_api/truvixx_streamline.export.h"

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Streamline 日志类型，隔离 sl::LogType 的 C++ ABI。
typedef enum : uint32_t
{
    TruvixxSlLogTypeInfo = 0,
    TruvixxSlLogTypeWarn = 1,
    TruvixxSlLogTypeError = 2,
} TruvixxSlLogType;

/// Rust 侧全局日志回调的签名。
///
/// C++ 只在 slInit 前保存该指针，后续 SL 日志事件通过它直接转发到 Rust。
/// 不携带 user_data：Rust 侧使用全局 OnceLock<SyncSender> 管理状态。
typedef void (*TruvixxSlLogCallback)(TruvixxSlLogType type, const char* message_utf8, uint32_t message_len);

/// Streamline 初始化描述。
///
/// 路径字段使用 null-terminated UTF-16。调用方（Rust）负责路径校验和目录创建，
/// C++ 侧不做额外检查，直接转交给 sl::Preferences。
typedef struct
{
    const uint16_t* plugin_dir_utf16;
    const uint16_t* log_dir_utf16;
    uint32_t show_console;
    uint32_t verbose_log;

    /// Rust 全局日志回调函数地址。该指针在 truvixx_sl_init 中保存到 file-scope static，
    /// 后续 SL callback 中直接调用。不可为 NULL。
    TruvixxSlLogCallback log_callback;
} TruvixxSlInitDesc;

/// 初始化 Streamline runtime 并加载 DLSS SR feature。
///
/// 返回 sl::Result 的原始 int32_t 值，0 表示成功。
/// C++ 不维护初始化状态；调用方（Rust）负责防重入。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_init(const TruvixxSlInitDesc* desc);

/// 关闭 Streamline runtime。
///
/// 返回 sl::Result 的原始 int32_t 值，0 表示成功。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_shutdown();

#ifdef __cplusplus
}
#endif
