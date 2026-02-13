#pragma once

#include <string>
#include <vector>

#include "vulkan/vulkan.hpp"

namespace truvixx {

/// Vulkan 逻辑设备封装
///
/// 创建和管理 Vulkan 逻辑设备以及相关的扩展功能
class GfxDevice {
public:
    GfxDevice(VkInstance instance,
              VkPhysicalDevice physicalDevice,
              const std::vector<VkDeviceQueueCreateInfo>& queueCreateInfos);

    ~GfxDevice();

    // 禁止拷贝
    GfxDevice(const GfxDevice&) = delete;
    GfxDevice& operator=(const GfxDevice&) = delete;

    // 允许移动
    GfxDevice(GfxDevice&& other) noexcept;
    GfxDevice& operator=(GfxDevice&& other) noexcept;

    [[nodiscard]] VkDevice handle() const { return m_device; }

    /// 获取指定队列族和索引的队列
    [[nodiscard]] VkQueue getQueue(uint32_t queueFamilyIndex, uint32_t queueIndex) const;

    /// 等待设备空闲
    void waitIdle() const;

    /// 设置 Vulkan 对象的调试名称
    void setObjectDebugName(uint64_t objectHandle, VkObjectType objectType, const std::string& name) const;

    template <typename T>
    void setObjectDebugName(T handle, const std::string& name) const;

private:
    /// 获取必须启用的设备扩展
    static std::vector<const char*> basicDeviceExts();

    /// 设置设备特性
    static VkPhysicalDeviceFeatures basicDeviceFeatures();

private:
    VkDevice m_device = VK_NULL_HANDLE;

    // 扩展函数指针
    PFN_vkSetDebugUtilsObjectNameEXT m_vkSetDebugUtilsObjectName = nullptr;
};

// 模板特化声明
template <>
void GfxDevice::setObjectDebugName<VkInstance>(VkInstance handle, const std::string& name) const;

template <>
void GfxDevice::setObjectDebugName<VkPhysicalDevice>(VkPhysicalDevice handle, const std::string& name) const;

template <>
void GfxDevice::setObjectDebugName<VkDevice>(VkDevice handle, const std::string& name) const;

template <>
void GfxDevice::setObjectDebugName<VkQueue>(VkQueue handle, const std::string& name) const;

template <>
void GfxDevice::setObjectDebugName<VkSwapchainKHR>(VkSwapchainKHR handle, const std::string& name) const;

template <>
void GfxDevice::setObjectDebugName<VkImage>(VkImage handle, const std::string& name) const;

template <>
void GfxDevice::setObjectDebugName<VkImageView>(VkImageView handle, const std::string& name) const;

} // namespace truvixx
