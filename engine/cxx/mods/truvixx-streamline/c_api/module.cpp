#include "TruvixxStreamline/c_api/module.h"

#include "TruvixxUtils/string.hpp"

#include <Windows.h>
#include <vulkan/vulkan.h>

#include <sl.h>
#include <sl_dlss.h>
#include <sl_helpers_vk.h>

#include <array>
#include <cstddef>
#include <cstring>
#include <cstdint>
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
    // 通用 Streamline 入口在 slInit 前就从 interposer DLL 解析出来；
    // DLSS 专属入口需要通过 slGetFeatureFunction 在 feature load 后懒解析。
    PFun_slIsFeatureSupported* sl_is_feature_supported = nullptr;
    PFun_slGetFeatureRequirements* sl_get_feature_requirements = nullptr;
    PFun_slGetFeatureFunction* sl_get_feature_function = nullptr;
    PFun_slGetNewFrameToken* sl_get_new_frame_token = nullptr;
    PFun_slSetConstants* sl_set_constants = nullptr;
    PFun_slSetTagForFrame* sl_set_tag_for_frame = nullptr;
    PFun_slEvaluateFeature* sl_evaluate_feature = nullptr;
    PFun_slFreeResources* sl_free_resources = nullptr;
    PFun_slSetVulkanInfo* sl_set_vulkan_info = nullptr;

    PFun_slDLSSGetOptimalSettings* sl_dlss_get_optimal_settings = nullptr;
    PFun_slDLSSSetOptions* sl_dlss_set_options = nullptr;

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
        sl_is_feature_supported = nullptr;
        sl_get_feature_requirements = nullptr;
        sl_get_feature_function = nullptr;
        sl_get_new_frame_token = nullptr;
        sl_set_constants = nullptr;
        sl_set_tag_for_frame = nullptr;
        sl_evaluate_feature = nullptr;
        sl_free_resources = nullptr;
        sl_set_vulkan_info = nullptr;
        sl_dlss_get_optimal_settings = nullptr;
        sl_dlss_set_options = nullptr;
        if (module)
        {
            FreeLibrary(module);
            module = nullptr;
        }
    }
};

SlApi g_sl_api;

void emit_wrapper_log(TruvixxSlLogType type, const std::string& message);

template<typename T>
T* resolve_required_export(HMODULE module, const char* name)
{
    // 所有基础入口都视为必需；缺任一导出说明当前 DLL 与 SDK 头文件不匹配。
    auto* function = reinterpret_cast<T*>(GetProcAddress(module, name));
    if (!function)
    {
        const DWORD error_code = GetLastError();
        emit_wrapper_log(
            TruvixxSlLogTypeError,
            "Failed to resolve " + std::string(name) + " from Streamline interposer DLL (" +
                StringUtils::win32_error_message(error_code) + ")"
        );
    }
    return function;
}

template<typename T>
T to_vk_handle(uint64_t raw)
{
    // Rust 侧只传 raw handle 数值，C++ 层在进入 Streamline 前恢复为 Vulkan handle 类型。
    return reinterpret_cast<T>(static_cast<uintptr_t>(raw));
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
    auto* sl_init = resolve_required_export<PFun_slInit>(module, "slInit");
    auto* sl_shutdown = resolve_required_export<PFun_slShutdown>(module, "slShutdown");
    auto* sl_is_feature_supported = resolve_required_export<PFun_slIsFeatureSupported>(module, "slIsFeatureSupported");
    auto* sl_get_feature_requirements =
        resolve_required_export<PFun_slGetFeatureRequirements>(module, "slGetFeatureRequirements");
    auto* sl_get_feature_function = resolve_required_export<PFun_slGetFeatureFunction>(module, "slGetFeatureFunction");
    auto* sl_get_new_frame_token = resolve_required_export<PFun_slGetNewFrameToken>(module, "slGetNewFrameToken");
    auto* sl_set_constants = resolve_required_export<PFun_slSetConstants>(module, "slSetConstants");
    auto* sl_set_tag_for_frame = resolve_required_export<PFun_slSetTagForFrame>(module, "slSetTagForFrame");
    auto* sl_evaluate_feature = resolve_required_export<PFun_slEvaluateFeature>(module, "slEvaluateFeature");
    auto* sl_free_resources = resolve_required_export<PFun_slFreeResources>(module, "slFreeResources");
    auto* sl_set_vulkan_info = resolve_required_export<PFun_slSetVulkanInfo>(module, "slSetVulkanInfo");
    if (!sl_init || !sl_shutdown || !sl_is_feature_supported || !sl_get_feature_requirements ||
        !sl_get_feature_function || !sl_get_new_frame_token || !sl_set_constants || !sl_set_tag_for_frame ||
        !sl_evaluate_feature || !sl_free_resources || !sl_set_vulkan_info)
    {
        FreeLibrary(module);
        return sl::Result::eErrorMissingOrInvalidAPI;
    }

    g_sl_api.module = module;
    g_sl_api.sl_init = sl_init;
    g_sl_api.sl_shutdown = sl_shutdown;
    g_sl_api.sl_is_feature_supported = sl_is_feature_supported;
    g_sl_api.sl_get_feature_requirements = sl_get_feature_requirements;
    g_sl_api.sl_get_feature_function = sl_get_feature_function;
    g_sl_api.sl_get_new_frame_token = sl_get_new_frame_token;
    g_sl_api.sl_set_constants = sl_set_constants;
    g_sl_api.sl_set_tag_for_frame = sl_set_tag_for_frame;
    g_sl_api.sl_evaluate_feature = sl_evaluate_feature;
    g_sl_api.sl_free_resources = sl_free_resources;
    g_sl_api.sl_set_vulkan_info = sl_set_vulkan_info;
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

sl::Boolean to_sl_boolean(const uint32_t value)
{
    return value == 0 ? sl::Boolean::eFalse : sl::Boolean::eTrue;
}

sl::DLSSMode to_sl_dlss_mode(const uint32_t mode)
{
    switch (mode)
    {
    case TruvixxSlDlssModeMaxPerformance:
        return sl::DLSSMode::eMaxPerformance;
    case TruvixxSlDlssModeBalanced:
        return sl::DLSSMode::eBalanced;
    case TruvixxSlDlssModeMaxQuality:
        return sl::DLSSMode::eMaxQuality;
    case TruvixxSlDlssModeUltraPerformance:
        return sl::DLSSMode::eUltraPerformance;
    case TruvixxSlDlssModeDlaa:
        return sl::DLSSMode::eDLAA;
    case TruvixxSlDlssModeOff:
    default:
        return sl::DLSSMode::eOff;
    }
}

sl::DLSSOptions make_dlss_options(const TruvixxSlDlssOptions& src)
{
    sl::DLSSOptions options{};
    options.mode = to_sl_dlss_mode(src.mode);
    options.outputWidth = src.output_width;
    options.outputHeight = src.output_height;
    options.preExposure = src.pre_exposure;
    options.exposureScale = src.exposure_scale;
    options.colorBuffersHDR = to_sl_boolean(src.color_buffers_hdr);
    // 当前 SR 接入没有单独曝光链路，也不把 alpha 当作可重建信号；先显式关闭，避免
    // 默认值随 SDK 变化影响画面契约。
    options.useAutoExposure = sl::Boolean::eFalse;
    options.alphaUpscalingEnabled = sl::Boolean::eFalse;
    return options;
}

void copy_matrix(sl::float4x4& dst, const float* src)
{
    // Rust foundation 层已经按 row-major 展开矩阵，这里逐行复制，避免 C++ 侧再猜测 glam 布局。
    for (uint32_t row = 0; row < 4; ++row)
    {
        dst.row[row] = sl::float4(src[row * 4 + 0], src[row * 4 + 1], src[row * 4 + 2], src[row * 4 + 3]);
    }
}

sl::Constants make_constants(const TruvixxSlConstants& src)
{
    sl::Constants constants{};
    copy_matrix(constants.cameraViewToClip, src.camera_view_to_clip);
    copy_matrix(constants.clipToCameraView, src.clip_to_camera_view);
    copy_matrix(constants.clipToPrevClip, src.clip_to_prev_clip);
    copy_matrix(constants.prevClipToClip, src.prev_clip_to_clip);
    constants.jitterOffset = sl::float2(src.jitter_offset[0], src.jitter_offset[1]);
    constants.mvecScale = sl::float2(src.mvec_scale[0], src.mvec_scale[1]);
    constants.cameraPinholeOffset = sl::float2(0.0f, 0.0f);
    constants.cameraPos = sl::float3(src.camera_pos[0], src.camera_pos[1], src.camera_pos[2]);
    constants.cameraUp = sl::float3(src.camera_up[0], src.camera_up[1], src.camera_up[2]);
    constants.cameraRight = sl::float3(src.camera_right[0], src.camera_right[1], src.camera_right[2]);
    constants.cameraFwd = sl::float3(src.camera_fwd[0], src.camera_fwd[1], src.camera_fwd[2]);
    constants.cameraNear = src.camera_near;
    constants.cameraFar = src.camera_far;
    constants.cameraFOV = src.camera_fov;
    constants.cameraAspectRatio = src.camera_aspect_ratio;
    constants.motionVectorsInvalidValue = src.motion_vectors_invalid_value;
    constants.depthInverted = to_sl_boolean(src.depth_inverted);
    constants.cameraMotionIncluded = to_sl_boolean(src.camera_motion_included);
    constants.motionVectors3D = to_sl_boolean(src.motion_vectors_3d);
    constants.reset = to_sl_boolean(src.reset);
    constants.orthographicProjection = sl::Boolean::eFalse;
    constants.motionVectorsDilated = sl::Boolean::eFalse;
    constants.motionVectorsJittered = sl::Boolean::eFalse;
    return constants;
}

sl::Resource make_image_resource(const TruvixxSlImageResource& src)
{
    // Streamline Vulkan helper 需要 image、memory、view、当前 layout 和 native format。
    // 这些字段必须来自同一帧 RenderGraph 已经转换后的资源状态。
    sl::Resource resource{
        sl::ResourceType::eTex2d,
        to_vk_handle<VkImage>(src.image),
        to_vk_handle<VkDeviceMemory>(src.memory),
        to_vk_handle<VkImageView>(src.image_view),
        src.layout,
    };
    resource.width = src.width;
    resource.height = src.height;
    resource.nativeFormat = src.format;
    resource.mipLevels = src.mip_levels;
    resource.arrayLayers = src.array_layers;
    resource.flags = src.flags;
    resource.usage = src.usage;
    return resource;
}

sl::Result ensure_dlss_feature_api()
{
    if (!g_sl_api.is_loaded() || !g_sl_api.sl_get_feature_function)
    {
        return sl::Result::eErrorNotInitialized;
    }
    if (!g_sl_api.sl_dlss_get_optimal_settings)
    {
        // DLSS 函数不直接从 interposer DLL 导出；必须在 kFeatureDLSS 被加载后通过
        // slGetFeatureFunction 查询，并缓存到进程级函数表。
        void* function = nullptr;
        const sl::Result result =
            g_sl_api.sl_get_feature_function(sl::kFeatureDLSS, "slDLSSGetOptimalSettings", function);
        if (result != sl::Result::eOk)
        {
            return result;
        }
        g_sl_api.sl_dlss_get_optimal_settings = reinterpret_cast<PFun_slDLSSGetOptimalSettings*>(function);
    }
    if (!g_sl_api.sl_dlss_set_options)
    {
        void* function = nullptr;
        const sl::Result result = g_sl_api.sl_get_feature_function(sl::kFeatureDLSS, "slDLSSSetOptions", function);
        if (result != sl::Result::eOk)
        {
            return result;
        }
        g_sl_api.sl_dlss_set_options = reinterpret_cast<PFun_slDLSSSetOptions*>(function);
    }
    return sl::Result::eOk;
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

int32_t truvixx_sl_set_vulkan_info(const TruvixxSlVulkanInfo* info)
{
    if (!g_sl_api.is_loaded() || !g_sl_api.sl_set_vulkan_info || !info)
    {
        return static_cast<int32_t>(sl::Result::eErrorInvalidParameter);
    }

    sl::VulkanInfo vulkan_info{};
    vulkan_info.instance = to_vk_handle<VkInstance>(info->instance);
    vulkan_info.physicalDevice = to_vk_handle<VkPhysicalDevice>(info->physical_device);
    vulkan_info.device = to_vk_handle<VkDevice>(info->device);
    vulkan_info.graphicsQueueFamily = info->graphics_queue_family;
    vulkan_info.graphicsQueueIndex = info->graphics_queue_index;
    vulkan_info.computeQueueFamily = info->compute_queue_family;
    vulkan_info.computeQueueIndex = info->compute_queue_index;

    const sl::Result result = g_sl_api.sl_set_vulkan_info(vulkan_info);
    return static_cast<int32_t>(result);
}

int32_t truvixx_sl_dlss_query_support(uint64_t physical_device, TruvixxSlFeatureSupport* out_support)
{
    if (!g_sl_api.is_loaded() || !g_sl_api.sl_is_feature_supported || !g_sl_api.sl_get_feature_requirements ||
        !out_support)
    {
        return static_cast<int32_t>(sl::Result::eErrorInvalidParameter);
    }

    sl::FeatureRequirements requirements{};
    const sl::Result requirements_result = g_sl_api.sl_get_feature_requirements(sl::kFeatureDLSS, requirements);
    out_support->flags = static_cast<uint32_t>(requirements.flags);
    out_support->max_num_viewports = requirements.maxNumViewports;
    out_support->max_num_cpu_threads = requirements.maxNumCPUThreads;

    sl::AdapterInfo adapter_info{};
    adapter_info.vkPhysicalDevice = to_vk_handle<VkPhysicalDevice>(physical_device);
    const sl::Result support_result = g_sl_api.sl_is_feature_supported(sl::kFeatureDLSS, adapter_info);
    out_support->supported = support_result == sl::Result::eOk ? 1u : 0u;

    if (requirements_result != sl::Result::eOk)
    {
        return static_cast<int32_t>(requirements_result);
    }
    return static_cast<int32_t>(support_result);
}

int32_t truvixx_sl_dlss_get_optimal_settings(
    const TruvixxSlDlssOptions* options,
    TruvixxSlDlssOptimalSettings* out_settings
)
{
    if (!options || !out_settings)
    {
        return static_cast<int32_t>(sl::Result::eErrorInvalidParameter);
    }
    const sl::Result api_result = ensure_dlss_feature_api();
    if (api_result != sl::Result::eOk)
    {
        return static_cast<int32_t>(api_result);
    }

    sl::DLSSOptimalSettings settings{};
    const sl::DLSSOptions dlss_options = make_dlss_options(*options);
    const sl::Result result = g_sl_api.sl_dlss_get_optimal_settings(dlss_options, settings);
    if (result == sl::Result::eOk)
    {
        out_settings->optimal_render_width = settings.optimalRenderWidth;
        out_settings->optimal_render_height = settings.optimalRenderHeight;
        out_settings->optimal_sharpness = settings.optimalSharpness;
        out_settings->render_width_min = settings.renderWidthMin;
        out_settings->render_height_min = settings.renderHeightMin;
        out_settings->render_width_max = settings.renderWidthMax;
        out_settings->render_height_max = settings.renderHeightMax;
    }
    return static_cast<int32_t>(result);
}

int32_t truvixx_sl_dlss_set_options(uint32_t viewport_id, const TruvixxSlDlssOptions* options)
{
    if (!options)
    {
        return static_cast<int32_t>(sl::Result::eErrorInvalidParameter);
    }
    const sl::Result api_result = ensure_dlss_feature_api();
    if (api_result != sl::Result::eOk)
    {
        return static_cast<int32_t>(api_result);
    }

    const sl::ViewportHandle viewport{ viewport_id };
    const sl::DLSSOptions dlss_options = make_dlss_options(*options);
    const sl::Result result = g_sl_api.sl_dlss_set_options(viewport, dlss_options);
    return static_cast<int32_t>(result);
}

int32_t truvixx_sl_dlss_evaluate(const TruvixxSlDlssEvaluateDesc* desc)
{
    if (!g_sl_api.is_loaded() || !g_sl_api.sl_get_new_frame_token || !g_sl_api.sl_set_constants ||
        !g_sl_api.sl_set_tag_for_frame || !g_sl_api.sl_evaluate_feature || !desc || desc->command_buffer == 0)
    {
        return static_cast<int32_t>(sl::Result::eErrorInvalidParameter);
    }

    sl::FrameToken* frame = nullptr;
    // Streamline 使用 frame token 把 constants、resource tags 和 evaluate 绑定到同一帧。
    const sl::Result frame_result = g_sl_api.sl_get_new_frame_token(frame, &desc->frame_index);
    if (frame_result != sl::Result::eOk)
    {
        return static_cast<int32_t>(frame_result);
    }

    const sl::ViewportHandle viewport{ desc->viewport_id };
    const sl::Constants constants = make_constants(desc->constants);
    // common constants 必须在 resource tag/evaluate 前设置；reset 标记也在这里随帧进入 DLSS。
    const sl::Result constants_result = g_sl_api.sl_set_constants(constants, *frame, viewport);
    if (constants_result != sl::Result::eOk)
    {
        return static_cast<int32_t>(constants_result);
    }

    sl::Resource input_color = make_image_resource(desc->input_color);
    sl::Resource output_color = make_image_resource(desc->output_color);
    sl::Resource depth = make_image_resource(desc->depth_or_linear_depth);
    sl::Resource motion_vectors = make_image_resource(desc->motion_vectors);

    const sl::Extent render_extent{ 0, 0, desc->input_color.width, desc->input_color.height };
    const sl::Extent output_extent{ 0, 0, desc->output_color.width, desc->output_color.height };
    // 输入只保证有效到 evaluate；输出至少要活到后续 SDR/GUI/present 使用完成。
    // 当前 SR 路径使用 device depth，所以 depth tag 固定为 kBufferTypeDepth。
    std::array<sl::ResourceTag, 4> tags{
        sl::ResourceTag(&input_color, sl::kBufferTypeScalingInputColor, sl::ResourceLifecycle::eValidUntilEvaluate, &render_extent),
        sl::ResourceTag(&output_color, sl::kBufferTypeScalingOutputColor, sl::ResourceLifecycle::eValidUntilPresent, &output_extent),
        sl::ResourceTag(&depth, sl::kBufferTypeDepth, sl::ResourceLifecycle::eValidUntilEvaluate, &render_extent),
        sl::ResourceTag(&motion_vectors, sl::kBufferTypeMotionVectors, sl::ResourceLifecycle::eValidUntilEvaluate, &render_extent),
    };

    auto* command_buffer = to_vk_handle<sl::CommandBuffer*>(desc->command_buffer);
    // 使用 frame-based resource tagging，tag 操作记录在同一个 command buffer 上，
    // 这样 RenderGraph 负责的 layout transition 与 Streamline 内部命令保持顺序一致。
    const sl::Result tag_result =
        g_sl_api.sl_set_tag_for_frame(*frame, viewport, tags.data(), static_cast<uint32_t>(tags.size()), command_buffer);
    if (tag_result != sl::Result::eOk)
    {
        return static_cast<int32_t>(tag_result);
    }

    const sl::BaseStructure* inputs[] = { &viewport };
    // 当前 wrapper 只执行普通 SR；RR 后续应切换 feature id 和 tag 集合，而不是在这里追加第二次 SR。
    const sl::Result evaluate_result =
        g_sl_api.sl_evaluate_feature(sl::kFeatureDLSS, *frame, inputs, static_cast<uint32_t>(sizeof(inputs) / sizeof(inputs[0])), command_buffer);
    return static_cast<int32_t>(evaluate_result);
}

int32_t truvixx_sl_dlss_free_resources(uint32_t viewport_id)
{
    if (!g_sl_api.is_loaded() || !g_sl_api.sl_free_resources)
    {
        return static_cast<int32_t>(sl::Result::eErrorNotInitialized);
    }

    const sl::ViewportHandle viewport{ viewport_id };
    // 调用方需要先等待相关 GPU work 完成；wrapper 不在这里做 device idle。
    const sl::Result result = g_sl_api.sl_free_resources(sl::kFeatureDLSS, viewport);
    return static_cast<int32_t>(result);
}
