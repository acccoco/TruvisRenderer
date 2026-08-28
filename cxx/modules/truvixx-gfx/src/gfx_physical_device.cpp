#include "TruvixxGfx/gfx_physical_device.hpp"

#include <iostream>
#include <stdexcept>

namespace truvixx
{

GfxPhysicalDevice::GfxPhysicalDevice(VkInstance instance)
{
    uint32_t deviceCount = 0;
    vkEnumeratePhysicalDevices(instance, &deviceCount, nullptr);

    if (deviceCount == 0)
    {
        throw std::runtime_error("Failed to find GPUs with Vulkan support");
    }

    std::vector<VkPhysicalDevice> devices(deviceCount);
    vkEnumeratePhysicalDevices(instance, &deviceCount, devices.data());

    // 优先选择独立显卡
    VkPhysicalDevice selectedDevice = VK_NULL_HANDLE;
    for (const auto& device : devices)
    {
        VkPhysicalDeviceProperties props;
        vkGetPhysicalDeviceProperties(device, &props);

        if (props.deviceType == VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU)
        {
            selectedDevice = device;
            break;
        }

        if (selectedDevice == VK_NULL_HANDLE)
        {
            selectedDevice = device;
        }
    }

    init_physical_device(selectedDevice, instance);
}

void GfxPhysicalDevice::init_physical_device(VkPhysicalDevice pdevice, VkInstance instance)
{
    physical_device_ = pdevice;

    // 获取基础属性
    vkGetPhysicalDeviceProperties(pdevice, &basic_props_);
    std::cout << "Found GPU: " << basic_props_.deviceName << "\n";

    // 获取特性
    vkGetPhysicalDeviceFeatures(pdevice, &features_);

    // 获取内存属性
    vkGetPhysicalDeviceMemoryProperties(pdevice, &mem_props_);

    // 获取 ray tracing 和加速结构属性
    rt_pipeline_props_.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_RAY_TRACING_PIPELINE_PROPERTIES_KHR;
    rt_pipeline_props_.pNext = nullptr;

    acc_struct_props_.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ACCELERATION_STRUCTURE_PROPERTIES_KHR;
    acc_struct_props_.pNext = &rt_pipeline_props_;

    VkPhysicalDeviceProperties2 props2{};
    props2.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2;
    props2.pNext = &acc_struct_props_;
    vkGetPhysicalDeviceProperties2(pdevice, &props2);

    // 获取队列族属性
    uint32_t queueFamilyCount = 0;
    vkGetPhysicalDeviceQueueFamilyProperties(pdevice, &queueFamilyCount, nullptr);

    queue_family_props_.resize(queueFamilyCount);
    vkGetPhysicalDeviceQueueFamilyProperties(pdevice, &queueFamilyCount, queue_family_props_.data());

    std::cout << "Queue family properties:\n";
    for (uint32_t i = 0; i < queueFamilyCount; ++i)
    {
        const auto& props = queue_family_props_[i];
        std::cout << "\t[" << i << "] flags: " << props.queueFlags << ", count: " << props.queueCount << "\n";
    }

    // 查找 Graphics Queue Family (支持 Graphics, Compute, Transfer)
    auto gfxFamily = find_queue_family(
        "gfx",
        VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT | VK_QUEUE_TRANSFER_BIT,
        0
    );

    if (!gfxFamily)
    {
        throw std::runtime_error("Failed to find graphics queue family");
    }
    gfx_queue_family_ = std::move(*gfxFamily);

    // 查找 Compute Only Queue Family
    compute_queue_family_ = find_queue_family(
        "compute-only",
        VK_QUEUE_COMPUTE_BIT | VK_QUEUE_TRANSFER_BIT,
        VK_QUEUE_GRAPHICS_BIT
    );

    // 查找 Transfer Only Queue Family
    transfer_queue_family_ = find_queue_family(
        "transfer-only",
        VK_QUEUE_TRANSFER_BIT,
        VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT
    );
}

std::optional<GfxQueueFamily> GfxPhysicalDevice::find_queue_family(
    const std::string& name,
    VkQueueFlags include_flags,
    VkQueueFlags exclude_flags
) const
{
    for (uint32_t i = 0; i < static_cast<uint32_t>(queue_family_props_.size()); ++i)
    {
        const auto& props = queue_family_props_[i];

        // 检查是否包含所有 include_flags
        if ((props.queueFlags & include_flags) != include_flags)
        {
            continue;
        }

        // 检查是否不包含任何 exclude_flags
        if ((props.queueFlags & exclude_flags) != 0)
        {
            continue;
        }

        return GfxQueueFamily{
            .name = name,
            .queue_family_index = i,
            .queue_flags = props.queueFlags,
            .queue_count = props.queueCount,
        };
    }

    return std::nullopt;
}

} // namespace truvixx
