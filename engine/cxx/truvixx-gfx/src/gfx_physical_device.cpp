#include "TruvixxGfx/gfx_physical_device.hpp"

#include <iostream>
#include <stdexcept>

namespace truvixx {

GfxPhysicalDevice::GfxPhysicalDevice(VkInstance instance)
{
    uint32_t deviceCount = 0;
    vkEnumeratePhysicalDevices(instance, &deviceCount, nullptr);

    if (deviceCount == 0) {
        throw std::runtime_error("Failed to find GPUs with Vulkan support");
    }

    std::vector<VkPhysicalDevice> devices(deviceCount);
    vkEnumeratePhysicalDevices(instance, &deviceCount, devices.data());

    // 优先选择独立显卡
    VkPhysicalDevice selectedDevice = VK_NULL_HANDLE;
    for (const auto& device : devices) {
        VkPhysicalDeviceProperties props;
        vkGetPhysicalDeviceProperties(device, &props);

        if (props.deviceType == VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU) {
            selectedDevice = device;
            break;
        }

        if (selectedDevice == VK_NULL_HANDLE) {
            selectedDevice = device;
        }
    }

    initPhysicalDevice(selectedDevice, instance);
}

void GfxPhysicalDevice::initPhysicalDevice(VkPhysicalDevice pdevice, VkInstance instance)
{
    m_physicalDevice = pdevice;

    // 获取基础属性
    vkGetPhysicalDeviceProperties(pdevice, &m_basicProps);
    std::cout << "Found GPU: " << m_basicProps.deviceName << "\n";

    // 获取特性
    vkGetPhysicalDeviceFeatures(pdevice, &m_features);

    // 获取内存属性
    vkGetPhysicalDeviceMemoryProperties(pdevice, &m_memProps);

    // 获取 ray tracing 和加速结构属性
    m_rtPipelineProps.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_RAY_TRACING_PIPELINE_PROPERTIES_KHR;
    m_rtPipelineProps.pNext = nullptr;

    m_accStructProps.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ACCELERATION_STRUCTURE_PROPERTIES_KHR;
    m_accStructProps.pNext = &m_rtPipelineProps;

    VkPhysicalDeviceProperties2 props2{};
    props2.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2;
    props2.pNext = &m_accStructProps;
    vkGetPhysicalDeviceProperties2(pdevice, &props2);

    // 获取队列族属性
    uint32_t queueFamilyCount = 0;
    vkGetPhysicalDeviceQueueFamilyProperties(pdevice, &queueFamilyCount, nullptr);

    m_queueFamilyProps.resize(queueFamilyCount);
    vkGetPhysicalDeviceQueueFamilyProperties(pdevice, &queueFamilyCount, m_queueFamilyProps.data());

    std::cout << "Queue family properties:\n";
    for (uint32_t i = 0; i < queueFamilyCount; ++i) {
        const auto& props = m_queueFamilyProps[i];
        std::cout << "\t[" << i << "] flags: " << props.queueFlags << ", count: " << props.queueCount << "\n";
    }

    // 查找 Graphics Queue Family (支持 Graphics, Compute, Transfer)
    auto gfxFamily = findQueueFamily(
        "gfx",
        VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT | VK_QUEUE_TRANSFER_BIT,
        0);

    if (!gfxFamily) {
        throw std::runtime_error("Failed to find graphics queue family");
    }
    m_gfxQueueFamily = std::move(*gfxFamily);

    // 查找 Compute Only Queue Family
    m_computeQueueFamily = findQueueFamily(
        "compute-only",
        VK_QUEUE_COMPUTE_BIT | VK_QUEUE_TRANSFER_BIT,
        VK_QUEUE_GRAPHICS_BIT);

    // 查找 Transfer Only Queue Family
    m_transferQueueFamily = findQueueFamily(
        "transfer-only",
        VK_QUEUE_TRANSFER_BIT,
        VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT);
}

std::optional<GfxQueueFamily> GfxPhysicalDevice::findQueueFamily(
    const std::string& name,
    VkQueueFlags includeFlags,
    VkQueueFlags excludeFlags) const
{
    for (uint32_t i = 0; i < static_cast<uint32_t>(m_queueFamilyProps.size()); ++i) {
        const auto& props = m_queueFamilyProps[i];

        // 检查是否包含所有 includeFlags
        if ((props.queueFlags & includeFlags) != includeFlags) {
            continue;
        }

        // 检查是否不包含任何 excludeFlags
        if ((props.queueFlags & excludeFlags) != 0) {
            continue;
        }

        return GfxQueueFamily{
            .name = name,
            .queueFamilyIndex = i,
            .queueFlags = props.queueFlags,
            .queueCount = props.queueCount,
        };
    }

    return std::nullopt;
}

} // namespace truvixx
