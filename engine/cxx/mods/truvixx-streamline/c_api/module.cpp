#include "TruvixxStreamline/c_api/module.h"

#include <sl.h>

#include <array>
#include <cstddef>
#include <cstring>

namespace
{

constexpr char kTruvisEngineVersion[] = "Truvis";

// NGX custom engine 的 Project ID，必须是稳定的 GUID-like 字符串。
// 正式发布若拿到 NVIDIA 分配的 applicationId，再统一切换身份策略。
constexpr char kTruvisNgxProjectId[] = "e390aa77-d1d4-42c5-be02-07095506af72";

// Rust 侧传入的全局日志回调地址。
// 在 truvixx_sl_init 中写入一次，之后只读。写入发生在 slInit 之前，
// 因此 SL callback 第一次触发时该指针已经有效，无需额外同步。
TruvixxSlLogCallback g_rust_log_callback = nullptr;

TruvixxSlLogType to_truvixx_log_type(const sl::LogType type)
{
    switch (type)
    {
    case sl::LogType::eWarn:
        return TruvixxSlLogTypeWarn;
    case sl::LogType::eError:
        return TruvixxSlLogTypeError;
    case sl::LogType::eInfo:
    default:
        return TruvixxSlLogTypeInfo;
    }
}

// 注册给 Streamline 的日志回调。只做枚举转换并转发到 Rust 全局函数。
void sl_log_callback(const sl::LogType type, const char* msg)
{
    if (!msg || !g_rust_log_callback)
    {
        return;
    }

    g_rust_log_callback(to_truvixx_log_type(type), msg, static_cast<uint32_t>(std::strlen(msg)));
}

} // namespace

int32_t truvixx_sl_init(const TruvixxSlInitDesc* desc)
{
    if (!desc || !desc->log_callback || !desc->plugin_dir_utf16)
    {
        return static_cast<int32_t>(sl::Result::eErrorInvalidParameter);
    }

    g_rust_log_callback = desc->log_callback;

    const wchar_t* plugin_dir = reinterpret_cast<const wchar_t*>(desc->plugin_dir_utf16);
    const wchar_t* log_dir = reinterpret_cast<const wchar_t*>(desc->log_dir_utf16);
    const bool show_console = desc->show_console != 0;
    const bool verbose_log = desc->verbose_log != 0;

    const wchar_t* plugin_paths[] = {plugin_dir};
    const std::array<sl::Feature, 1> features_to_load = {sl::kFeatureDLSS};

    sl::Preferences preferences{};
    preferences.showConsole = show_console;
    preferences.logLevel = verbose_log ? sl::LogLevel::eVerbose : sl::LogLevel::eDefault;
    preferences.pathsToPlugins = plugin_paths;
    preferences.numPathsToPlugins = static_cast<uint32_t>(std::size(plugin_paths));
    preferences.pathToLogsAndData = log_dir;
    preferences.logMessageCallback = sl_log_callback;
    preferences.flags = sl::PreferenceFlags::eDisableCLStateTracking | sl::PreferenceFlags::eUseFrameBasedResourceTagging;
    preferences.featuresToLoad = features_to_load.data();
    preferences.numFeaturesToLoad = static_cast<uint32_t>(features_to_load.size());
    preferences.engine = sl::EngineType::eCustom;
    preferences.engineVersion = kTruvisEngineVersion;
    preferences.projectId = kTruvisNgxProjectId;
    preferences.renderAPI = sl::RenderAPI::eVulkan;

    return static_cast<int32_t>(slInit(preferences));
}

int32_t truvixx_sl_shutdown()
{
    return static_cast<int32_t>(slShutdown());
}
