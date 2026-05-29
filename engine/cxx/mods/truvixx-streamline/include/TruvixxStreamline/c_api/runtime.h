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

/// Streamline feature 请求位。Rust 侧负责按运行配置组合这些 bit，
/// C++ 侧只把稳定 C ABI flag 翻译成 Streamline SDK 的 feature id。
typedef enum : uint32_t
{
    TruvixxSlFeatureFlagDlss = 1u << 0,
    TruvixxSlFeatureFlagImgui = 1u << 1,
} TruvixxSlFeatureFlag;

/// Rust 侧全局日志回调的签名。
///
/// C++ 只在 slInit 前保存该指针，后续 SL 日志事件通过它直接转发到 Rust。
/// Rust 侧使用全局 OnceLock<LogSenderState> 管理状态，不向 C++ 暴露对象指针。
typedef void (*TruvixxSlLogCallback)(TruvixxSlLogType type, const char* message_utf8, uint32_t message_len);

/// Streamline 初始化描述。
///
/// 路径字段使用 null-terminated UTF-16。调用方（Rust）负责路径校验和目录创建，
/// C++ 侧不做额外检查，直接转交给 sl::Preferences。
typedef struct
{
    /// Streamline plugin/runtime 搜索目录，传给 `sl::Preferences::pathsToPlugins`。
    /// 该目录通常也是 executable 所在目录，但语义上仍是 plugin 搜索根。
    const uint16_t* plugin_dir_utf16;

    /// Rust 侧解析出的 `sl.interposer.dll` 绝对路径。
    /// C++ 只使用该路径显式加载 SL DLL；Rust 后续也用同一路径创建 Vulkan Entry，
    /// 从而保证 SL API 调用与 Vulkan dispatch 链路落在同一个 interposer 模块上。
    const uint16_t* interposer_dll_path_utf16;

    /// Streamline 日志与诊断数据目录，传给 `sl::Preferences::pathToLogsAndData`。
    const uint16_t* log_dir_utf16;
    uint32_t show_console;
    uint32_t verbose_log;

    /// 请求加载的 Streamline feature bitset，取值来自 `TruvixxSlFeatureFlag`。
    uint32_t feature_flags;

    /// Rust 全局日志回调函数地址。该指针在 truvixx_sl_init 中保存到 file-scope static，
    /// 后续 SL callback 中直接调用。不可为 NULL。
    TruvixxSlLogCallback log_callback;
} TruvixxSlInitDesc;

/// 按 `TruvixxSlInitDesc::feature_flags` 初始化 Streamline runtime 并加载请求的 feature。
///
/// 返回 sl::Result 的原始 int32_t 值，0 表示成功。
/// C++ 侧在 init/shutdown 之间持有 DLL handle 与函数表；调用方（Rust）负责防重入。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_init(const TruvixxSlInitDesc* desc);

/// 关闭 Streamline runtime。
///
/// 返回 sl::Result 的原始 int32_t 值，0 表示成功。
TRUVIXX_STREAMLINE_API int32_t truvixx_sl_shutdown();

#ifdef __cplusplus
}
#endif
