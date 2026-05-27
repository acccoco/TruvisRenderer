#include "TruvixxStreamline/c_api/module.h"

#include "TruvixxUtils/path.hpp"
#include "TruvixxUtils/string.hpp"

#include <sl.h>

#include <array>
#include <cstring>
#include <mutex>
#include <sstream>
#include <string>

#define NOMINMAX
#define WIN32_LEAN_AND_MEAN
#include <Windows.h>

namespace
{

// Streamline init/shutdown 是进程级生命周期，不绑定某个 Rust 对象实例。
// 这里用全局状态把 C API 做成明确的单例：重复 init 会返回错误，shutdown 只允许执行一次。
//
// 使用 recursive_mutex 是因为 slInit 期间可能同步触发日志回调，而日志回调也会更新
// g_last_error。普通 mutex 会在这种重入路径上自锁。
std::recursive_mutex g_state_mutex;
bool g_initialized = false;
std::string g_last_error;
TruvixxSlLogCallback g_log_callback = nullptr;
void* g_log_user_data = nullptr;

void set_last_error(std::string message)
{
    g_last_error = std::move(message);
}

void set_last_error_from_sl(const char* action, const sl::Result result)
{
    std::ostringstream oss;
    oss << action << " failed, sl::Result=" << static_cast<int>(result);
    set_last_error(oss.str());
}

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

void clear_log_callback()
{
    g_log_callback = nullptr;
    g_log_user_data = nullptr;
}

void sl_log_callback(const sl::LogType type, const char* msg)
{
    if (!msg)
    {
        return;
    }

    TruvixxSlLogCallback log_callback = nullptr;
    void* log_user_data = nullptr;

    {
        std::lock_guard lock(g_state_mutex);
        if (type == sl::LogType::eError)
        {
            set_last_error(std::string("Streamline log error: ") + msg);
        }

        // Rust callback 的生命周期由 StreamlineRuntime 持有。这里在锁内复制裸指针，
        // 随后在锁外调用，避免 Rust 日志桥或 channel 操作反向阻塞 SL 生命周期锁。
        log_callback = g_log_callback;
        log_user_data = g_log_user_data;
    }

    if (!log_callback)
    {
        return;
    }

    log_callback(
        to_truvixx_log_type(type),
        msg,
        static_cast<uint32_t>(std::strlen(msg)),
        static_cast<uint32_t>(GetCurrentThreadId()),
        log_user_data
    );
}

} // namespace

TruvixxSlResult truvixx_sl_init(const TruvixxSlInitDesc* desc)
{
    std::lock_guard lock(g_state_mutex);

    if (g_initialized)
    {
        set_last_error("Streamline runtime has already been initialized.");
        return TruvixxSlResultAlreadyInitialized;
    }

    const std::wstring plugin_dir_default = truvixx::utils::PathUtils::current_executable_dir();
    const std::wstring log_dir_default = truvixx::utils::PathUtils::default_temp_streamline_log_dir();
    const wchar_t* plugin_dir =
        truvixx::utils::StringUtils::utf16_ptr_or_default(desc ? desc->plugin_dir_utf16 : nullptr, plugin_dir_default);
    const wchar_t* log_dir =
        truvixx::utils::StringUtils::utf16_ptr_or_default(desc ? desc->log_dir_utf16 : nullptr, log_dir_default);
    const bool show_console = desc ? desc->show_console != 0 : false;
    const bool verbose_log = desc ? desc->verbose_log != 0 : false;
    g_log_callback = desc ? desc->log_callback : nullptr;
    g_log_user_data = desc ? desc->log_user_data : nullptr;

    if (!plugin_dir || plugin_dir[0] == L'\0')
    {
        clear_log_callback();
        set_last_error("Streamline plugin dir is empty.");
        return TruvixxSlResultInvalidArgument;
    }

    // slInit 不负责为 pathToLogsAndData 创建目录。提前创建可以把路径错误转成
    // wrapper 层明确的 InvalidArgument，而不是让 SL 返回更难定位的初始化失败。
    std::string log_dir_error;
    if (!truvixx::utils::PathUtils::ensure_directory(log_dir, &log_dir_error))
    {
        clear_log_callback();
        set_last_error(std::move(log_dir_error));
        return TruvixxSlResultInvalidArgument;
    }

    const wchar_t* plugin_paths[] = { plugin_dir };
    const std::array<sl::Feature, 1> features_to_load = { sl::kFeatureDLSS };

    // 第一阶段只初始化 SL runtime 和 DLSS SR plugin：
    // - renderAPI 必须是 Vulkan，确保后续 feature requirements 与 Vulkan backend 对齐。
    // - 不启用 eUseManualHooking，因为 Vulkan loader 路径会走 sl.interposer.dll。
    // - 启用 eUseFrameBasedResourceTagging，为后续按 frame token tag resource 做准备。
    // - 不加载 DLSS-G / DLSS-RR / Reflex / NIS，避免无用 DLL 和 hook 进入当前进程。
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
    preferences.engineVersion = "Truvis";
    preferences.projectId = "truvis-dlss-streamline";
    preferences.renderAPI = sl::RenderAPI::eVulkan;

    const sl::Result result = slInit(preferences);
    if (result != sl::Result::eOk)
    {
        clear_log_callback();
        set_last_error_from_sl("slInit", result);
        return TruvixxSlResultStreamlineError;
    }

    g_initialized = true;
    set_last_error({});
    return TruvixxSlResultOk;
}

TruvixxSlResult truvixx_sl_shutdown()
{
    std::lock_guard lock(g_state_mutex);

    if (!g_initialized)
    {
        set_last_error("Streamline runtime has not been initialized.");
        return TruvixxSlResultNotInitialized;
    }

    const sl::Result result = slShutdown();
    clear_log_callback();
    if (result != sl::Result::eOk)
    {
        set_last_error_from_sl("slShutdown", result);
        return TruvixxSlResultStreamlineError;
    }

    g_initialized = false;
    set_last_error({});
    return TruvixxSlResultOk;
}

uint32_t truvixx_sl_is_initialized()
{
    std::lock_guard lock(g_state_mutex);
    return g_initialized ? 1u : 0u;
}

const char* truvixx_sl_last_error_utf8()
{
    std::lock_guard lock(g_state_mutex);
    return g_last_error.c_str();
}
