#pragma once

#include <string>
#include <optional>

#include "vulkan/vulkan.hpp"

namespace truvixx
{

/// Queue Family 信息
struct GfxQueueFamily
{
    std::string name;
    uint32_t queue_family_index;
    VkQueueFlags queue_flags;
    uint32_t queue_count;
};

/// 物理设备（GPU）封装
///
/// 选择并保存物理设备信息，包括设备属性、队列族等
struct GfxPhysicalDevice
{
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

    [[nodiscard]] VkPhysicalDevice handle() const { return physical_device_; }
    [[nodiscard]] const VkPhysicalDeviceProperties& properties() const { return basic_props_; }
    [[nodiscard]] const VkPhysicalDeviceMemoryProperties& memory_properties() const { return mem_props_; }
    [[nodiscard]] const GfxQueueFamily& gfx_queue_family() const { return gfx_queue_family_; }
    [[nodiscard]] const std::optional<GfxQueueFamily>& compute_queue_family() const { return compute_queue_family_; }
    [[nodiscard]] const std::optional<GfxQueueFamily>& transfer_queue_family() const { return transfer_queue_family_; }

    [[nodiscard]] bool is_discrete_gpu() const { return basic_props_.deviceType == VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU; }

private:
    void init_physical_device(VkPhysicalDevice pdevice, VkInstance instance);

    /// 查找符合条件的 queue family
    std::optional<GfxQueueFamily> find_queue_family(
        const std::string& name,
        VkQueueFlags include_flags,
        VkQueueFlags exclude_flags
    ) const;

private:
    VkPhysicalDevice physical_device_ = VK_NULL_HANDLE;

    VkPhysicalDeviceFeatures features_{};
    VkPhysicalDeviceProperties basic_props_{};
    VkPhysicalDeviceMemoryProperties mem_props_{};
    VkPhysicalDeviceRayTracingPipelinePropertiesKHR rt_pipeline_props_{};
    VkPhysicalDeviceAccelerationStructurePropertiesKHR acc_struct_props_{};

    std::vector<VkQueueFamilyProperties> queue_family_props_;

    GfxQueueFamily gfx_queue_family_;
    std::optional<GfxQueueFamily> compute_queue_family_;
    std::optional<GfxQueueFamily> transfer_queue_family_;
};

} // namespace truvixx
