#pragma once

#include <string>
#include <optional>

#include "vulkan/vulkan.hpp"

namespace truvixx {

/// Queue Family 信息
struct GfxQueueFamily {
    std::string name;
    uint32_t queueFamilyIndex;
    VkQueueFlags queueFlags;
    uint32_t queueCount;
};

/// 物理设备（GPU）封装
///
/// 选择并保存物理设备信息，包括设备属性、队列族等
class GfxPhysicalDevice {
public:
    /// 创建一个新的物理设备实例
    /// 优先选择独立显卡，如果没有则选择第一个可用的显卡
    explicit GfxPhysicalDevice(VkInstance instance);

    ~GfxPhysicalDevice() = default;

    // 禁止拷贝
    GfxPhysicalDevice(const GfxPhysicalDevice&) = delete;
    GfxPhysicalDevice& operator=(const GfxPhysicalDevice&) = delete;

    // 允许移动
    GfxPhysicalDevice(GfxPhysicalDevice&&) noexcept = default;
    GfxPhysicalDevice& operator=(GfxPhysicalDevice&&) noexcept = default;

    [[nodiscard]] VkPhysicalDevice handle() const { return m_physicalDevice; }
    [[nodiscard]] const VkPhysicalDeviceProperties& properties() const { return m_basicProps; }
    [[nodiscard]] const VkPhysicalDeviceMemoryProperties& memoryProperties() const { return m_memProps; }
    [[nodiscard]] const GfxQueueFamily& gfxQueueFamily() const { return m_gfxQueueFamily; }
    [[nodiscard]] const std::optional<GfxQueueFamily>& computeQueueFamily() const { return m_computeQueueFamily; }
    [[nodiscard]] const std::optional<GfxQueueFamily>& transferQueueFamily() const { return m_transferQueueFamily; }

    [[nodiscard]] bool isDiscreteGpu() const { return m_basicProps.deviceType == VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU; }

private:
    void initPhysicalDevice(VkPhysicalDevice pdevice, VkInstance instance);

    /// 查找符合条件的 queue family
    std::optional<GfxQueueFamily> findQueueFamily(
        const std::string& name,
        VkQueueFlags includeFlags,
        VkQueueFlags excludeFlags) const;

private:
    VkPhysicalDevice m_physicalDevice = VK_NULL_HANDLE;

    VkPhysicalDeviceFeatures m_features{};
    VkPhysicalDeviceProperties m_basicProps{};
    VkPhysicalDeviceMemoryProperties m_memProps{};
    VkPhysicalDeviceRayTracingPipelinePropertiesKHR m_rtPipelineProps{};
    VkPhysicalDeviceAccelerationStructurePropertiesKHR m_accStructProps{};

    std::vector<VkQueueFamilyProperties> m_queueFamilyProps;

    GfxQueueFamily m_gfxQueueFamily;
    std::optional<GfxQueueFamily> m_computeQueueFamily;
    std::optional<GfxQueueFamily> m_transferQueueFamily;
};

} // namespace truvixx
