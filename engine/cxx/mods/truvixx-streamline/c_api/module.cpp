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
// 这把锁同时保护四类状态：
// - g_initialized：进程级 SL runtime 是否已经初始化。
// - g_last_error：最近一次 wrapper 或 SL 错误文本。
// - g_log_callback / g_log_user_data：Rust 日志桥的裸指针入口。
// - Rust user_data 的使用窗口：shutdown 也必须先拿到同一把锁，才能清空 callback 并返回给 Rust drop。
//
// 使用 recursive_mutex 是因为 slInit 期间可能同步触发日志回调，而日志回调也会更新
// g_last_error。普通 mutex 会在这种重入路径上自锁。
//
// 约定：本文件里任何读写 g_* 全局状态的代码都必须先持有这把锁。不要把单个字段拆成
// atomic，因为 callback 函数指针和 user_data 必须作为一组一致状态读写，initialized 状态
// 也必须和 callback 注册/清理顺序保持一致。
std::recursive_mutex g_state_mutex;

// 当前进程是否已经成功完成 slInit。
//
// 只有 slInit 返回 eOk 后才会置为 true；只有 slShutdown 返回 eOk 后才会置为 false。
// 该状态用于把 Streamline 的进程级生命周期暴露成明确的单例 C API：重复 init 和未 init
// shutdown 都会被 wrapper 拦截，而不是继续交给 Streamline 内部处理。
bool g_initialized = false;

// 最近一次 wrapper 或 Streamline 错误文本。
//
// Rust 侧通过 truvixx_sl_last_error_utf8 读取该字符串。返回的 const char* 指向本 string
// 内部缓冲区，只适合作为“下一次 wrapper 调用修改错误文本前”的临时诊断视图；调用方需要
// 长期保存时应立即复制。它不是日志流，完整 SL 日志走 g_log_callback 转发到 Rust log facade。
std::string g_last_error;

// Rust 日志桥的回调函数指针。
//
// 该指针在 truvixx_sl_init 中随 desc 注册，在 init 失败或 shutdown 后清空。Streamline
// callback 可能来自任意线程，因此读写都必须受 g_state_mutex 保护。它和 g_log_user_data
// 是一对不可拆分的状态：只要 callback 非空，user_data 就必须仍指向 Rust 侧有效日志桥状态。
TruvixxSlLogCallback g_log_callback = nullptr;

// Rust 日志桥的 opaque user_data。
//
// C++ 侧只保存并原样回传，不解引用、不接管所有权。实际对象由 Rust 的 StreamlineLogBridge
// 持有；StreamlineRuntime drop 会先调用 truvixx_sl_shutdown，再释放该对象。sl_log_callback
// 在 g_state_mutex 内调用 Rust callback，正是为了让“使用 user_data”和“shutdown 后 Rust 释放
// user_data”之间有同一把锁提供顺序关系。
void* g_log_user_data = nullptr;

// 更新最近错误文本。调用者必须已经持有 g_state_mutex。
//
// 这里不单独加锁，避免在 init/shutdown/log callback 已经持锁的路径上重复表达锁策略。
// 锁的所有权集中在外层 C API 或 Streamline callback 入口处，更容易看清生命周期边界。
void set_last_error(std::string message)
{
    g_last_error = std::move(message);
}

// 将 Streamline C++ ABI 的 sl::Result 压缩成 wrapper 的 UTF-8 诊断文本。
//
// C ABI 不直接暴露 sl::Result，Rust 侧只看到稳定的 TruvixxSlResult；具体 SL 数值留在
// last_error 里用于排查 SDK/plugin/driver 初始化问题。调用者必须已经持有 g_state_mutex。
void set_last_error_from_sl(const char* action, const sl::Result result)
{
    std::ostringstream oss;
    oss << action << " failed, sl::Result=" << static_cast<int>(result);
    set_last_error(oss.str());
}

// 把 Streamline 的日志枚举映射到项目稳定 C ABI 枚举。
//
// 这个函数不读取全局状态，不需要持锁。未知或未来新增的 SL 日志类型按 Info 处理，避免
// 因 SDK 扩展让日志桥变成硬失败点。
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

// 清空 Rust 日志桥入口。调用者必须已经持有 g_state_mutex。
//
// callback 和 user_data 必须一起清空，不能只清一个字段。否则 Streamline callback 可能观察到
// 半更新状态：有 callback 但 user_data 已失效，或有 user_data 但 callback 不再表示有效入口。
void clear_log_callback()
{
    g_log_callback = nullptr;
    g_log_user_data = nullptr;
}

// Streamline 注册的内部日志回调。
//
// 该函数可能在 slInit、slShutdown 或后续 Vulkan interposer 调用栈中同步触发，线程来源不由
// wrapper 控制。它的职责只是在 C++ 侧记录 error 文本，并把日志事件转发到 Rust 的轻量回调；
// 最终日志 IO 由 Rust 的 drain 线程完成。
void sl_log_callback(const sl::LogType type, const char* msg)
{
    if (!msg)
    {
        return;
    }

    std::lock_guard lock(g_state_mutex);

    if (type == sl::LogType::eError)
    {
        set_last_error(std::string("Streamline log error: ") + msg);
    }

    if (g_log_callback)
    {
        // 这里故意在生命周期锁内调用 Rust callback，而不是复制裸指针后解锁再调用。
        //
        // Rust 侧 `StreamlineLogBridge` 持有 `g_log_user_data` 指向的 Box。`StreamlineRuntime`
        // drop 时会先调用 `truvixx_sl_shutdown()`，再释放该 Box。shutdown 也持有
        // `g_state_mutex`，因此只要 callback 在锁内完成，就不会出现“C++ 已复制旧 user_data，
        // Rust 随后释放 Box，C++ 再使用悬空指针”的窗口。
        //
        // 这个方案成立的前提是 Rust callback 足够薄：它只复制本次 SL 消息并 try_send 到
        // bounded queue，不调用 SL API，不做最终日志 IO，也不等待 drain 线程。因此把它放在
        // 生命周期锁内不会把复杂日志系统或渲染路径反向锁进 Streamline init/shutdown。
        g_log_callback(
            to_truvixx_log_type(type),
            msg,
            static_cast<uint32_t>(std::strlen(msg)),
            static_cast<uint32_t>(GetCurrentThreadId()),
            g_log_user_data
        );
    }
}

} // namespace

// 初始化进程级 Streamline runtime。
//
// 线程安全：整个函数持有 g_state_mutex，保证 init/shutdown 互斥，也保证 callback 指针注册和
// g_initialized 状态更新具有一致顺序。desc 中的 UTF-16 指针只在本次同步调用期间使用，不在
// C++ wrapper 中保存；唯一会保存的 desc 内容是 Rust 日志 callback 与 user_data。
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

    // 先注册 Rust 日志桥，再调用 slInit。Streamline 在 init 期间就可能同步输出日志，
    // 包括 plugin 路径错误、driver/NGX 状态或 feature 加载失败等诊断信息。若后续参数检查
    // 或 slInit 失败，必须调用 clear_log_callback，把这对裸指针恢复到空状态。
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

// 关闭进程级 Streamline runtime。
//
// 线程安全：shutdown 持有 g_state_mutex，因此它会等待正在执行的 sl_log_callback 返回；
// sl_log_callback 也会在同一把锁内使用 g_log_user_data。Rust 的 StreamlineRuntime drop 在
// 本函数返回后才释放 StreamlineLogBridge，因此不会和 C++ 日志回调并发使用同一块 user_data。
TruvixxSlResult truvixx_sl_shutdown()
{
    std::lock_guard lock(g_state_mutex);

    if (!g_initialized)
    {
        set_last_error("Streamline runtime has not been initialized.");
        return TruvixxSlResultNotInitialized;
    }

    const sl::Result result = slShutdown();

    // slShutdown 期间仍可能输出日志，所以必须等它返回后再清空 Rust callback。清空后即使
    // Streamline 后续异常触发 callback，wrapper 也只会更新 last_error，不再触碰 Rust user_data。
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

// 查询 wrapper 眼中的初始化状态。
//
// 这里返回的是本 C API wrapper 的状态，而不是重新向 Streamline 查询。读取也持锁，是为了和
// init/shutdown 的状态变更保持同一同步边界。
uint32_t truvixx_sl_is_initialized()
{
    std::lock_guard lock(g_state_mutex);
    return g_initialized ? 1u : 0u;
}

// 返回最近错误文本的 UTF-8 指针。
//
// 指针所有权归 g_last_error，不得由调用方释放。由于返回后锁会释放，下一次 wrapper 调用可能
// 改写 g_last_error 并让该指针失效；Rust 侧应像当前 binding 那样立即复制成 String。
const char* truvixx_sl_last_error_utf8()
{
    std::lock_guard lock(g_state_mutex);
    return g_last_error.c_str();
}
