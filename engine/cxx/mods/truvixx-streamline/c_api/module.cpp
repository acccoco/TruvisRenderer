#include "TruvixxStreamline/c_api/module.h"

#include "TruvixxUtils/string.hpp"

#include <Windows.h>

#include <sl.h>

#include <array>
#include <cstddef>
#include <cstring>
#include <string>

namespace
{

using truvixx::utils::StringUtils;

constexpr char kTruvisEngineVersion[] = "Truvis";

// NGX custom engine 的 Project ID，必须是稳定的 GUID-like 字符串。
// 正式发布若拿到 NVIDIA 分配的 applicationId，再统一切换身份策略。
constexpr char kTruvisNgxProjectId[] = "e390aa77-d1d4-42c5-be02-07095506af72";

// Rust 侧传入的全局日志回调地址。
// 在 truvixx_sl_init 中写入一次，之后只读。写入发生在 slInit 之前，
// 因此 SL callback 第一次触发时该指针已经有效，无需额外同步。
TruvixxSlLogCallback g_rust_log_callback = nullptr;

// Streamline 是进程级 runtime。这里显式保存 DLL handle 和入口函数表，
// 让 C API wrapper 可以在 `truvixx_sl_init` 时才加载 `sl.interposer.dll`，
// 避免链接期依赖让 Windows loader 在进程启动阶段提前加载 SL runtime。
struct SlApi
{
    HMODULE module = nullptr;
    PFun_slInit* sl_init = nullptr;
    PFun_slShutdown* sl_shutdown = nullptr;

    bool is_loaded() const
    {
        return module != nullptr && sl_init != nullptr && sl_shutdown != nullptr;
    }

    void reset()
    {
        // reset 同时服务两条路径：
        // 1. slInit 失败后释放刚加载的 DLL，避免半初始化状态残留；
        // 2. slShutdown 返回后释放 C++ wrapper 持有的 DLL 引用。
        // 若 Rust/ash 也加载了同一路径的 DLL，Windows 引用计数会保证这里不会提前卸载仍在使用的模块。
        sl_init = nullptr;
        sl_shutdown = nullptr;
        if (module)
        {
            FreeLibrary(module);
            module = nullptr;
        }
    }
};

SlApi g_sl_api;

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

void emit_wrapper_log(const TruvixxSlLogType type, const std::string& message)
{
    // 动态加载失败可能发生在 slInit 之前，此时还没有 Streamline 自己的 log callback。
    // Rust 在调用 C API 前已经安装全局日志回调，因此 wrapper 诊断也走同一条异步日志链路。
    if (!g_rust_log_callback)
    {
        return;
    }

    g_rust_log_callback(type, message.c_str(), static_cast<uint32_t>(message.size()));
}

sl::Result load_streamline_api(const wchar_t* interposer_dll_path)
{
    // 必须使用 Rust 传入的绝对路径，而不是依赖 DLL 搜索路径。
    // 这样 C++ 调用 slInit/slShutdown 的模块，与后续 ash::Entry::load_from 使用的
    // `sl.interposer.dll` 是同一份文件，避免出现两套 SL interposer 或加载顺序不透明的问题。
    if (!interposer_dll_path)
    {
        emit_wrapper_log(TruvixxSlLogTypeError, "Streamline interposer DLL path is null.");
        return sl::Result::eErrorInvalidParameter;
    }

    if (g_sl_api.module)
    {
        emit_wrapper_log(TruvixxSlLogTypeError, "Streamline interposer DLL is already loaded.");
        return sl::Result::eErrorInvalidState;
    }

    HMODULE module = LoadLibraryW(interposer_dll_path);
    if (!module)
    {
        const DWORD error_code = GetLastError();
        emit_wrapper_log(
            TruvixxSlLogTypeError,
            "Failed to load Streamline interposer DLL: " + StringUtils::to_utf8(interposer_dll_path) + " (" +
                StringUtils::win32_error_message(error_code) + ")"
        );
        return sl::Result::eErrorIO;
    }

    // Streamline C API 导出名是未修饰的 C symbol；动态解析时不会应用头文件里的默认参数，
    // 因此调用 slInit 时需要显式传入 sl::kSDKVersion。
    auto* sl_init = reinterpret_cast<PFun_slInit*>(GetProcAddress(module, "slInit"));
    auto* sl_shutdown = reinterpret_cast<PFun_slShutdown*>(GetProcAddress(module, "slShutdown"));
    if (!sl_init || !sl_shutdown)
    {
        const DWORD error_code = GetLastError();
        emit_wrapper_log(
            TruvixxSlLogTypeError,
            "Failed to resolve slInit/slShutdown from Streamline interposer DLL: " +
                StringUtils::to_utf8(interposer_dll_path) + " (" + StringUtils::win32_error_message(error_code) + ")"
        );
        FreeLibrary(module);
        return sl::Result::eErrorMissingOrInvalidAPI;
    }

    g_sl_api.module = module;
    g_sl_api.sl_init = sl_init;
    g_sl_api.sl_shutdown = sl_shutdown;
    return sl::Result::eOk;
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
    if (!desc || !desc->log_callback || !desc->plugin_dir_utf16 || !desc->interposer_dll_path_utf16)
    {
        return static_cast<int32_t>(sl::Result::eErrorInvalidParameter);
    }

    g_rust_log_callback = desc->log_callback;

    const wchar_t* plugin_dir = reinterpret_cast<const wchar_t*>(desc->plugin_dir_utf16);
    const wchar_t* interposer_dll_path = reinterpret_cast<const wchar_t*>(desc->interposer_dll_path_utf16);
    const wchar_t* log_dir = reinterpret_cast<const wchar_t*>(desc->log_dir_utf16);
    const bool show_console = desc->show_console != 0;
    const bool verbose_log = desc->verbose_log != 0;
    const uint32_t feature_flags = desc->feature_flags;

    // 先加载函数表，再组装 Preferences。这样即使 DLL 不存在或导出不匹配，
    // 也能在进入 Streamline runtime 前返回明确错误，并通过 Rust callback 输出诊断。
    const sl::Result load_result = load_streamline_api(interposer_dll_path);
    if (load_result != sl::Result::eOk)
    {
        g_rust_log_callback = nullptr;
        return static_cast<int32_t>(load_result);
    }

    const wchar_t* plugin_paths[] = { plugin_dir };
    std::array<sl::Feature, 2> features_to_load{};
    uint32_t feature_count = 0;
    if ((feature_flags & TruvixxSlFeatureFlagDlss) != 0)
    {
        features_to_load[feature_count++] = sl::kFeatureDLSS;
    }
    if ((feature_flags & TruvixxSlFeatureFlagImgui) != 0)
    {
        features_to_load[feature_count++] = sl::kFeatureImGUI;
    }
    if (feature_count == 0)
    {
        emit_wrapper_log(TruvixxSlLogTypeError, "Streamline feature flags did not request any known feature.");
        g_sl_api.reset();
        g_rust_log_callback = nullptr;
        return static_cast<int32_t>(sl::Result::eErrorInvalidParameter);
    }

    sl::Preferences preferences{};
    preferences.showConsole = show_console;
    preferences.logLevel = verbose_log ? sl::LogLevel::eVerbose : sl::LogLevel::eDefault;
    preferences.pathsToPlugins = plugin_paths;
    preferences.numPathsToPlugins = static_cast<uint32_t>(std::size(plugin_paths));
    preferences.pathToLogsAndData = log_dir;
    preferences.logMessageCallback = sl_log_callback;
    preferences.flags = sl::PreferenceFlags::eDisableCLStateTracking | sl::PreferenceFlags::eUseFrameBasedResourceTagging;
    preferences.featuresToLoad = features_to_load.data();
    preferences.numFeaturesToLoad = feature_count;
    preferences.engine = sl::EngineType::eCustom;
    preferences.engineVersion = kTruvisEngineVersion;
    preferences.projectId = kTruvisNgxProjectId;
    preferences.renderAPI = sl::RenderAPI::eVulkan;

    // PFun_slInit 没有 C++ 默认参数语义，必须显式传入当前 SDK 版本。
    // 版本不匹配时交给 Streamline 返回具体 sl::Result，Rust 侧再统一转成启动失败。
    const sl::Result init_result = g_sl_api.sl_init(preferences, sl::kSDKVersion);
    if (init_result != sl::Result::eOk)
    {
        g_sl_api.reset();
        g_rust_log_callback = nullptr;
    }
    return static_cast<int32_t>(init_result);
}

int32_t truvixx_sl_shutdown()
{
    if (!g_sl_api.is_loaded())
    {
        g_rust_log_callback = nullptr;
        return static_cast<int32_t>(sl::Result::eErrorNotInitialized);
    }

    const sl::Result shutdown_result = g_sl_api.sl_shutdown();
    // slShutdown 之后再释放 wrapper 持有的 DLL 引用。Gfx 会在 Vulkan root 销毁前 drop
    // StreamlineRuntime；如果 ash Entry 仍持有同一 DLL，Windows 引用计数会延后真正卸载。
    g_sl_api.reset();
    g_rust_log_callback = nullptr;
    return static_cast<int32_t>(shutdown_result);
}
