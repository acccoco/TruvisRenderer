#include "TruvixxDlssNr/c_api/module.h"
#include "TruvixxUtils/ngx_identity.hpp"

#include <Windows.h>
#include <vulkan/vulkan.h>
#include <nvsdk_ngx_helpers_dlssnr_vk.h>

#include <atomic>
#include <cmath>
#include <cstring>
#include <memory>
#include <string>

namespace
{
/// 路径和 discovery 中的借用指针只在同步调用期间有效。
struct NrDiscovery
{
    std::wstring directory;
    const wchar_t* search;
    NVSDK_NGX_FeatureCommonInfo common{};
    NVSDK_NGX_FeatureDiscoveryInfo info{};

    explicit NrDiscovery(const TruvixxNrInitDesc& d)
        : directory(reinterpret_cast<const wchar_t*>(d.dll_directory_utf16))
        , search(directory.c_str())
    {
        common.PathListInfo = { &search, 1 };
        common.LoggingInfo.MinimumLoggingLevel = NVSDK_NGX_LOGGING_LEVEL_ON;
        info.SDKVersion = NVSDK_NGX_Version_API;
        info.FeatureID = NVSDK_NGX_Feature_DLSSNR;
        info.Identifier.IdentifierType = NVSDK_NGX_Application_Identifier_Type_Project_Id;
        info.Identifier.v.ProjectDesc = { truvixx::utils::NgxIdentity::project_id, NVSDK_NGX_ENGINE_TYPE_CUSTOM, truvixx::utils::NgxIdentity::engine_version };
        info.ApplicationDataPath = reinterpret_cast<const wchar_t*>(d.log_directory_utf16);
        info.FeatureInfo = &common;
    }
    bool dll_exists() const
    {
        auto path = directory + L"\\nvngx_dlssnr.dll";
        auto attributes = GetFileAttributesW(path.c_str());
        return attributes != INVALID_FILE_ATTRIBUTES && !(attributes & FILE_ATTRIBUTE_DIRECTORY);
    }
};

struct NrApi
{
    inline static std::atomic_bool occupied{ false };
    static TruvixxNrStatus result(uint32_t stage, NVSDK_NGX_Result r = NVSDK_NGX_Result_Success)
    {
        return { stage, static_cast<uint32_t>(r), TruvixxNrErrorNone };
    }
    static TruvixxNrStatus error(uint32_t stage, uint32_t e) { return { stage, 0, e }; }
    static bool success(TruvixxNrStatus s) { return s.error == 0 && !NVSDK_NGX_FAILED(static_cast<NVSDK_NGX_Result>(s.ngx_result)); }
    template <class F>
    static TruvixxNrStatus boundary(uint32_t stage, F&& f) noexcept
    {
        try
        {
            return f();
        } catch (...)
        {
            return error(stage, TruvixxNrErrorException);
        }
    }
    static bool valid_paths(const TruvixxNrInitDesc* d)
    {
        return d && d->dll_directory_utf16 && d->log_directory_utf16;
    }
    static TruvixxNrStatus extensions(const TruvixxNrInitDesc* d, uint32_t device, TruvixxNrExtension* out, uint32_t* count)
    {
        constexpr auto stage = TruvixxNrStageRequirements;
        if (!valid_paths(d) || !count || device > 1 || (device && (!d->instance || !d->physical_device))) return error(stage, TruvixxNrErrorArgument);
        NrDiscovery discovery(*d);
        if (!discovery.dll_exists()) return error(stage, TruvixxNrErrorMissingDll);
        uint32_t required = 0;
        VkExtensionProperties* properties = nullptr;
        auto r = device ? NVSDK_NGX_VULKAN_GetFeatureDeviceExtensionRequirements(reinterpret_cast<VkInstance>(d->instance), reinterpret_cast<VkPhysicalDevice>(d->physical_device), &discovery.info, &required, &properties) : NVSDK_NGX_VULKAN_GetFeatureInstanceExtensionRequirements(&discovery.info, &required, &properties);
        if (NVSDK_NGX_FAILED(r)) return result(stage, r);
        const auto capacity = *count;
        *count = required;
        if (!out) return result(stage);
        if (capacity < required) return error(stage, TruvixxNrErrorBufferTooSmall);
        for (uint32_t i = 0; i < required; ++i)
        {
            std::memcpy(out[i].name, properties[i].extensionName, sizeof(out[i].name));
            out[i].name[255] = 0;
        }
        return result(stage);
    }
};
} // namespace

/// 唯一 NR 会话 owner，不拥有 Vulkan 对象，也不借用 Streamline 私有参数块。
/// shutdown 是显式操作；C ABI 调用须在创建线程串行执行。
struct TruvixxNrSession
{
    DWORD thread = GetCurrentThreadId();
    VkDevice device{};
    NVSDK_NGX_Parameter* parameters{};
    NVSDK_NGX_Handle* feature{};
    uint32_t width{}, height{};

    TruvixxNrStatus check(uint32_t stage) const
    {
        return thread == GetCurrentThreadId() ? NrApi::result(stage) : NrApi::error(stage, TruvixxNrErrorThread);
    }
    TruvixxNrStatus capabilities(TruvixxNrCapabilities* out)
    {
        constexpr auto stage = TruvixxNrStageCapability;
        if (!NrApi::success(check(stage))) return check(stage);
        if (!out) return NrApi::error(stage, TruvixxNrErrorArgument);
        *out = {};
        if (!parameters)
        {
            auto r = NVSDK_NGX_VULKAN_GetCapabilityParameters(&parameters);
            if (NVSDK_NGX_FAILED(r)) return NrApi::result(stage, r);
        }
        out->available = query(NVSDK_NGX_Parameter_DLSSNR_Available);
        out->needs_updated_driver = query(NVSDK_NGX_Parameter_DLSSNR_NeedsUpdatedDriver);
        out->feature_init_result = query(NVSDK_NGX_Parameter_DLSSNR_FeatureInitResult);
        out->min_driver_major = query_unsigned(NVSDK_NGX_Parameter_DLSSNR_MinDriverVersionMajor);
        out->min_driver_minor = query_unsigned(NVSDK_NGX_Parameter_DLSSNR_MinDriverVersionMinor);
        return NrApi::result(stage);
    }
    TruvixxNrQuery query(const char* name)
    {
        int value = 0;
        auto r = parameters->Get(name, &value);
        return { static_cast<uint32_t>(r), static_cast<uint32_t>(value) };
    }
    TruvixxNrQuery query_unsigned(const char* name)
    {
        unsigned int value = 0;
        auto r = parameters->Get(name, &value);
        return { static_cast<uint32_t>(r), value };
    }
    TruvixxNrStatus create(uint64_t command, uint32_t w, uint32_t h)
    {
        constexpr auto stage = TruvixxNrStageCreate;
        if (!NrApi::success(check(stage))) return check(stage);
        if (feature) return NrApi::error(stage, TruvixxNrErrorState);
        if (!command || !w || !h) return NrApi::error(stage, TruvixxNrErrorArgument);
        TruvixxNrCapabilities caps{};
        auto s = capabilities(&caps);
        if (!NrApi::success(s)) return s;
        for (auto q : { caps.available, caps.needs_updated_driver, caps.feature_init_result })
            if (NVSDK_NGX_FAILED(static_cast<NVSDK_NGX_Result>(q.ngx_result))) return NrApi::result(stage, static_cast<NVSDK_NGX_Result>(q.ngx_result));
        if (!caps.available.value || caps.needs_updated_driver.value) return NrApi::error(stage, TruvixxNrErrorUnavailable);
        if (NVSDK_NGX_FAILED(static_cast<NVSDK_NGX_Result>(caps.feature_init_result.value))) return NrApi::result(stage, static_cast<NVSDK_NGX_Result>(caps.feature_init_result.value));
        parameters->Set(NVSDK_NGX_Parameter_DLSSNR_Hint_Render_Preset, int(NVSDK_NGX_DLSSNR_Hint_Render_Preset_B));
        NVSDK_NGX_DLSSNR_Create_Params desc{ w, h };
        auto r = NGX_VULKAN_CREATE_DLSSNR_EXT1(device, reinterpret_cast<VkCommandBuffer>(command), 1, 1, &feature, parameters, &desc);
        if (!NVSDK_NGX_FAILED(r))
        {
            width = w;
            height = h;
        }
        /// SDK 失败时若仍返回 feature，保留给 release，不丢失销毁责任。
        return NrApi::result(stage, r);
    }
    bool valid_image(const TruvixxNrImage& image, VkFormat format) const
    {
        return image.image && image.image_view && image.width == width && image.height == height && image.format == uint32_t(format) && image.layout == VK_IMAGE_LAYOUT_GENERAL;
    }
    static NVSDK_NGX_Resource_VK resource(const TruvixxNrImage& image, bool writable)
    {
        return NVSDK_NGX_Create_ImageView_Resource_VK(reinterpret_cast<VkImageView>(image.image_view), reinterpret_cast<VkImage>(image.image), { VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1 }, static_cast<VkFormat>(image.format), image.width, image.height, writable);
    }
    TruvixxNrStatus evaluate(uint64_t command, const TruvixxNrEvaluateDesc* d)
    {
        constexpr auto stage = TruvixxNrStageEvaluate;
        if (!NrApi::success(check(stage))) return check(stage);
        if (!feature) return NrApi::error(stage, TruvixxNrErrorState);
        if (!command || !d || d->reset > 1 || !std::isfinite(d->jitter_x) || !std::isfinite(d->jitter_y) || !std::isfinite(d->motion_scale_x) || !std::isfinite(d->motion_scale_y) || !valid_image(d->color, VK_FORMAT_R16G16B16A16_SFLOAT) || !valid_image(d->output, VK_FORMAT_R16G16B16A16_SFLOAT) || !valid_image(d->depth, VK_FORMAT_R32_SFLOAT) || !valid_image(d->motion_vectors, VK_FORMAT_R16G16_SFLOAT) || !valid_image(d->control_mask, VK_FORMAT_R8G8B8A8_UNORM) || d->color.image == d->output.image)
            return NrApi::error(stage, TruvixxNrErrorArgument);
        auto color = resource(d->color, false), depth = resource(d->depth, false), motion = resource(d->motion_vectors, false);
        auto mask = resource(d->control_mask, false), output = resource(d->output, true);
        NVSDK_NGX_VK_DLSSNR_Eval_Params p{};
        p.pInColor = &color;
        p.pInDepth = &depth;
        p.pInMVec = &motion;
        p.pInControlMask = &mask;
        p.pInOutput = &output;
        p.InReset = int(d->reset);
        p.InEnabled = 1;
        p.InIntensity = 1;
        p.InLocalToneStrength = 0.3f;
        p.InLocalStructureStrength = 0.7f;
        p.InMVecScaleX = d->motion_scale_x;
        p.InMVecScaleY = d->motion_scale_y;
        p.InColorSubrectSize = p.InOutputSubrectSize = p.InDepthSubrectSize = p.InMVecSubrectSize = p.InControlMaskSubrectSize = { width, height };
        parameters->Set(NVSDK_NGX_Parameter_Jitter_Offset_X, d->jitter_x);
        parameters->Set(NVSDK_NGX_Parameter_Jitter_Offset_Y, d->jitter_y);
        return NrApi::result(stage, NGX_VULKAN_EVALUATE_DLSSNR_EXT(reinterpret_cast<VkCommandBuffer>(command), feature, parameters, &p));
    }
    TruvixxNrStatus release()
    {
        constexpr auto stage = TruvixxNrStageRelease;
        if (!NrApi::success(check(stage))) return check(stage);
        if (!feature) return NrApi::result(stage);
        auto r = NVSDK_NGX_VULKAN_ReleaseFeature(feature);
        if (!NVSDK_NGX_FAILED(r))
        {
            feature = nullptr;
            width = height = 0;
        }
        return NrApi::result(stage, r);
    }
    TruvixxNrStatus shutdown()
    {
        constexpr auto stage = TruvixxNrStageShutdown;
        if (!NrApi::success(check(stage))) return check(stage);
        if (feature) return NrApi::error(stage, TruvixxNrErrorState);
        if (parameters)
        {
            auto r = NVSDK_NGX_VULKAN_DestroyParameters(parameters);
            if (NVSDK_NGX_FAILED(r)) return NrApi::result(stage, r);
            parameters = nullptr;
        }
        return NrApi::result(stage, NVSDK_NGX_VULKAN_Shutdown1(device));
    }
    static TruvixxNrStatus init(const TruvixxNrInitDesc* d, TruvixxNrSession** out)
    {
        constexpr auto stage = TruvixxNrStageInit;
        if (!out) return NrApi::error(stage, TruvixxNrErrorArgument);
        *out = nullptr;
        if (!NrApi::valid_paths(d) || !d->instance || !d->physical_device || !d->device || !d->get_instance_proc_addr || !d->get_device_proc_addr)
            return NrApi::error(stage, TruvixxNrErrorArgument);
        if ((d->enabled_features & TruvixxNrRequiredFeatures) != TruvixxNrRequiredFeatures)
            return NrApi::error(stage, TruvixxNrErrorDeviceContract);
        NrDiscovery discovery(*d);
        if (!discovery.dll_exists()) return NrApi::error(stage, TruvixxNrErrorMissingDll);
        auto gipa = reinterpret_cast<PFN_vkGetInstanceProcAddr>(d->get_instance_proc_addr);
        auto gdpa = reinterpret_cast<PFN_vkGetDeviceProcAddr>(d->get_device_proc_addr);
        auto vkDevice = reinterpret_cast<VkDevice>(d->device);
        for (const char* name : { "vkCmdPipelineBarrier2", "vkCmdPushDescriptorSetKHR", "vkGetImageViewHandleNVX", "vkCreateCuModuleNVX", "vkGetBufferDeviceAddress" })
            if (!gdpa(vkDevice, name)) return NrApi::error(stage, TruvixxNrErrorDeviceContract);
        auto session = std::make_unique<TruvixxNrSession>();
        session->device = vkDevice;
        bool expected = false;
        if (!NrApi::occupied.compare_exchange_strong(expected, true)) return NrApi::error(stage, TruvixxNrErrorState);
        auto r = NVSDK_NGX_VULKAN_Init_with_ProjectID(truvixx::utils::NgxIdentity::project_id, NVSDK_NGX_ENGINE_TYPE_CUSTOM, truvixx::utils::NgxIdentity::engine_version, discovery.info.ApplicationDataPath, reinterpret_cast<VkInstance>(d->instance), reinterpret_cast<VkPhysicalDevice>(d->physical_device), vkDevice, gipa, gdpa, &discovery.common);
        if (NVSDK_NGX_FAILED(r))
        {
            NrApi::occupied = false;
            return NrApi::result(stage, r);
        }
        *out = session.release();
        return NrApi::result(stage);
    }
};

TruvixxNrStatus truvixx_nr_extensions(const TruvixxNrInitDesc* d, uint32_t device, TruvixxNrExtension* out, uint32_t* count)
{
    return NrApi::boundary(TruvixxNrStageRequirements, [&] { return NrApi::extensions(d, device, out, count); });
}
TruvixxNrStatus truvixx_nr_init(const TruvixxNrInitDesc* d, TruvixxNrSession** out)
{
    return NrApi::boundary(TruvixxNrStageInit, [&] { return TruvixxNrSession::init(d, out); });
}
TruvixxNrStatus truvixx_nr_capabilities(TruvixxNrSession* s, TruvixxNrCapabilities* out)
{
    return NrApi::boundary(TruvixxNrStageCapability, [&] { return s ? s->capabilities(out) : NrApi::error(TruvixxNrStageCapability, TruvixxNrErrorArgument); });
}
TruvixxNrStatus truvixx_nr_create(TruvixxNrSession* s, uint64_t command, uint32_t width, uint32_t height)
{
    return NrApi::boundary(TruvixxNrStageCreate, [&] { return s ? s->create(command, width, height) : NrApi::error(TruvixxNrStageCreate, TruvixxNrErrorArgument); });
}
TruvixxNrStatus truvixx_nr_evaluate(TruvixxNrSession* s, uint64_t command, const TruvixxNrEvaluateDesc* d)
{
    return NrApi::boundary(TruvixxNrStageEvaluate, [&] { return s ? s->evaluate(command, d) : NrApi::error(TruvixxNrStageEvaluate, TruvixxNrErrorArgument); });
}
TruvixxNrStatus truvixx_nr_release(TruvixxNrSession* s)
{
    return NrApi::boundary(TruvixxNrStageRelease, [&] { return s ? s->release() : NrApi::error(TruvixxNrStageRelease, TruvixxNrErrorArgument); });
}
TruvixxNrStatus truvixx_nr_shutdown(TruvixxNrSession** s)
{
    return NrApi::boundary(TruvixxNrStageShutdown, [&] {
        if (!s || !*s) return NrApi::error(TruvixxNrStageShutdown, TruvixxNrErrorArgument);
        auto status = (*s)->shutdown();
        if (NrApi::success(status))
        {
            delete *s;
            *s = nullptr;
            NrApi::occupied = false;
        }
        return status;
    });
}
