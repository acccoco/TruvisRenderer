#pragma once

#include <string>
#include <vector>

#include "vulkan/vulkan.hpp"

namespace truvixx
{

/// Vulkan 逻辑设备封装
///
/// 创建和管理 Vulkan 逻辑设备以及相关的扩展功能
struct GfxDevice
{
public:
    GfxDevice(
        VkInstance instance,
        VkPhysicalDevice physical_device,
        const std::vector<VkDeviceQueueCreateInfo>& queue_create_infos
    );

    ~GfxDevice();

    // 禁止拷贝
    GfxDevice(const GfxDevice&) = delete;
    GfxDevice& operator=(const GfxDevice&) = delete;

    // 允许移动
    GfxDevice(GfxDevice&& other) noexcept;
    GfxDevice& operator=(GfxDevice&& other) noexcept;

    [[nodiscard]] VkDevice handle() const { return device_; }

    /// 获取指定队列族和索引的队列
    [[nodiscard]] VkQueue get_queue(uint32_t queueFamilyIndex, uint32_t queueIndex) const;

    /// 等待设备空闲
    void wait_idle() const;

    /// 设置 Vulkan 对象的调试名称
    void set_object_debug_name(uint64_t object_handle, VkObjectType object_type, const std::string& name) const;

    template <typename T>
    void set_object_debug_name(T handle, const std::string& name) const;

private:
    /// 获取必须启用的设备扩展
    static std::vector<const char*> basic_device_exts();

    /// 设置设备特性
    static VkPhysicalDeviceFeatures basic_device_features();

private:
    VkDevice device_ = VK_NULL_HANDLE;

    // 扩展函数指针
    PFN_vkSetDebugUtilsObjectNameEXT pfn_vkSetDebugUtilsObjectName = nullptr;
};

#pragma region 模板特化声明

template <>
void GfxDevice::set_object_debug_name<VkInstance>(VkInstance handle, const std::string& name) const;

template <>
void GfxDevice::set_object_debug_name<VkPhysicalDevice>(VkPhysicalDevice handle, const std::string& name) const;

template <>
void GfxDevice::set_object_debug_name<VkDevice>(VkDevice handle, const std::string& name) const;

template <>
void GfxDevice::set_object_debug_name<VkQueue>(VkQueue handle, const std::string& name) const;

template <>
void GfxDevice::set_object_debug_name<VkSwapchainKHR>(VkSwapchainKHR handle, const std::string& name) const;

template <>
void GfxDevice::set_object_debug_name<VkImage>(VkImage handle, const std::string& name) const;

template <>
void GfxDevice::set_object_debug_name<VkImageView>(VkImageView handle, const std::string& name) const;

#pragma endregion

} // namespace truvixx
