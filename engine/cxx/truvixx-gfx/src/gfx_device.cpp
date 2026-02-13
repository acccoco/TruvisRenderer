#include "TruvixxGfx/gfx_device.hpp"

#include <iostream>
#include <stdexcept>

namespace truvixx {

GfxDevice::GfxDevice(VkInstance instance,
                     VkPhysicalDevice physicalDevice,
                     const std::vector<VkDeviceQueueCreateInfo>& queueCreateInfos)
{
    // 设备扩展
    auto deviceExts = basicDeviceExts();

    std::cout << "Device extensions:\n";
    for (const auto& ext : deviceExts) {
        std::cout << "\t" << ext << "\n";
    }

    // 设备特性
    VkPhysicalDeviceFeatures basicFeatures = basicDeviceFeatures();

    // 扩展特性链
    VkPhysicalDeviceDynamicRenderingFeatures dynamicRenderingFeatures{};
    dynamicRenderingFeatures.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DYNAMIC_RENDERING_FEATURES;
    dynamicRenderingFeatures.dynamicRendering = VK_TRUE;

    VkPhysicalDeviceBufferDeviceAddressFeatures bufferDeviceAddressFeatures{};
    bufferDeviceAddressFeatures.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_BUFFER_DEVICE_ADDRESS_FEATURES;
    bufferDeviceAddressFeatures.bufferDeviceAddress = VK_TRUE;
    bufferDeviceAddressFeatures.pNext = &dynamicRenderingFeatures;

    VkPhysicalDeviceRayTracingPipelineFeaturesKHR rtPipelineFeatures{};
    rtPipelineFeatures.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_RAY_TRACING_PIPELINE_FEATURES_KHR;
    rtPipelineFeatures.rayTracingPipeline = VK_TRUE;
    rtPipelineFeatures.pNext = &bufferDeviceAddressFeatures;

    VkPhysicalDeviceAccelerationStructureFeaturesKHR accelStructFeatures{};
    accelStructFeatures.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ACCELERATION_STRUCTURE_FEATURES_KHR;
    accelStructFeatures.accelerationStructure = VK_TRUE;
    accelStructFeatures.pNext = &rtPipelineFeatures;

    VkPhysicalDeviceRayQueryFeaturesKHR rayQueryFeatures{};
    rayQueryFeatures.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_RAY_QUERY_FEATURES_KHR;
    rayQueryFeatures.rayQuery = VK_TRUE;
    rayQueryFeatures.pNext = &accelStructFeatures;

    VkPhysicalDeviceSynchronization2Features sync2Features{};
    sync2Features.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SYNCHRONIZATION_2_FEATURES;
    sync2Features.synchronization2 = VK_TRUE;
    sync2Features.pNext = &rayQueryFeatures;

    VkPhysicalDeviceTimelineSemaphoreFeatures timelineSemaphoreFeatures{};
    timelineSemaphoreFeatures.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES;
    timelineSemaphoreFeatures.timelineSemaphore = VK_TRUE;
    timelineSemaphoreFeatures.pNext = &sync2Features;

    VkPhysicalDeviceDescriptorIndexingFeatures descriptorIndexingFeatures{};
    descriptorIndexingFeatures.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DESCRIPTOR_INDEXING_FEATURES;
    descriptorIndexingFeatures.descriptorBindingPartiallyBound = VK_TRUE;
    descriptorIndexingFeatures.runtimeDescriptorArray = VK_TRUE;
    descriptorIndexingFeatures.descriptorBindingSampledImageUpdateAfterBind = VK_TRUE;
    descriptorIndexingFeatures.descriptorBindingStorageImageUpdateAfterBind = VK_TRUE;
    descriptorIndexingFeatures.descriptorBindingVariableDescriptorCount = VK_TRUE;
    descriptorIndexingFeatures.pNext = &timelineSemaphoreFeatures;

    VkPhysicalDeviceHostQueryResetFeatures hostQueryResetFeatures{};
    hostQueryResetFeatures.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_HOST_QUERY_RESET_FEATURES;
    hostQueryResetFeatures.hostQueryReset = VK_TRUE;
    hostQueryResetFeatures.pNext = &descriptorIndexingFeatures;

    VkPhysicalDeviceFeatures2 allFeatures{};
    allFeatures.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2;
    allFeatures.features = basicFeatures;
    allFeatures.pNext = &hostQueryResetFeatures;

    // 创建设备
    VkDeviceCreateInfo createInfo{};
    createInfo.sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO;
    createInfo.queueCreateInfoCount = static_cast<uint32_t>(queueCreateInfos.size());
    createInfo.pQueueCreateInfos = queueCreateInfos.data();
    createInfo.enabledExtensionCount = static_cast<uint32_t>(deviceExts.size());
    createInfo.ppEnabledExtensionNames = deviceExts.data();
    createInfo.pNext = &allFeatures;

    VkResult result = vkCreateDevice(physicalDevice, &createInfo, nullptr, &m_device);
    if (result != VK_SUCCESS) {
        throw std::runtime_error("Failed to create logical device");
    }

    // 加载扩展函数
    m_vkSetDebugUtilsObjectName = reinterpret_cast<PFN_vkSetDebugUtilsObjectNameEXT>(
        vkGetDeviceProcAddr(m_device, "vkSetDebugUtilsObjectNameEXT"));
}

GfxDevice::~GfxDevice()
{
    if (m_device != VK_NULL_HANDLE) {
        std::cout << "Destroying GfxDevice\n";
        vkDestroyDevice(m_device, nullptr);
        m_device = VK_NULL_HANDLE;
    }
}

GfxDevice::GfxDevice(GfxDevice&& other) noexcept
    : m_device(other.m_device)
    , m_vkSetDebugUtilsObjectName(other.m_vkSetDebugUtilsObjectName)
{
    other.m_device = VK_NULL_HANDLE;
    other.m_vkSetDebugUtilsObjectName = nullptr;
}

GfxDevice& GfxDevice::operator=(GfxDevice&& other) noexcept
{
    if (this != &other) {
        if (m_device != VK_NULL_HANDLE) {
            vkDestroyDevice(m_device, nullptr);
        }
        m_device = other.m_device;
        m_vkSetDebugUtilsObjectName = other.m_vkSetDebugUtilsObjectName;
        other.m_device = VK_NULL_HANDLE;
        other.m_vkSetDebugUtilsObjectName = nullptr;
    }
    return *this;
}

VkQueue GfxDevice::getQueue(uint32_t queueFamilyIndex, uint32_t queueIndex) const
{
    VkQueue queue;
    vkGetDeviceQueue(m_device, queueFamilyIndex, queueIndex, &queue);
    return queue;
}

void GfxDevice::waitIdle() const
{
    vkDeviceWaitIdle(m_device);
}

void GfxDevice::setObjectDebugName(uint64_t objectHandle, VkObjectType objectType, const std::string& name) const
{
    if (m_vkSetDebugUtilsObjectName == nullptr) {
        return;
    }

    VkDebugUtilsObjectNameInfoEXT nameInfo{};
    nameInfo.sType = VK_STRUCTURE_TYPE_DEBUG_UTILS_OBJECT_NAME_INFO_EXT;
    nameInfo.objectType = objectType;
    nameInfo.objectHandle = objectHandle;
    nameInfo.pObjectName = name.c_str();

    m_vkSetDebugUtilsObjectName(m_device, &nameInfo);
}

template <>
void GfxDevice::setObjectDebugName<VkInstance>(VkInstance handle, const std::string& name) const
{
    setObjectDebugName(reinterpret_cast<uint64_t>(handle), VK_OBJECT_TYPE_INSTANCE, name);
}

template <>
void GfxDevice::setObjectDebugName<VkPhysicalDevice>(VkPhysicalDevice handle, const std::string& name) const
{
    setObjectDebugName(reinterpret_cast<uint64_t>(handle), VK_OBJECT_TYPE_PHYSICAL_DEVICE, name);
}

template <>
void GfxDevice::setObjectDebugName<VkDevice>(VkDevice handle, const std::string& name) const
{
    setObjectDebugName(reinterpret_cast<uint64_t>(handle), VK_OBJECT_TYPE_DEVICE, name);
}

template <>
void GfxDevice::setObjectDebugName<VkQueue>(VkQueue handle, const std::string& name) const
{
    setObjectDebugName(reinterpret_cast<uint64_t>(handle), VK_OBJECT_TYPE_QUEUE, name);
}

template <>
void GfxDevice::setObjectDebugName<VkSwapchainKHR>(VkSwapchainKHR handle, const std::string& name) const
{
    setObjectDebugName(reinterpret_cast<uint64_t>(handle), VK_OBJECT_TYPE_SWAPCHAIN_KHR, name);
}

template <>
void GfxDevice::setObjectDebugName<VkImage>(VkImage handle, const std::string& name) const
{
    setObjectDebugName(reinterpret_cast<uint64_t>(handle), VK_OBJECT_TYPE_IMAGE, name);
}

template <>
void GfxDevice::setObjectDebugName<VkImageView>(VkImageView handle, const std::string& name) const
{
    setObjectDebugName(reinterpret_cast<uint64_t>(handle), VK_OBJECT_TYPE_IMAGE_VIEW, name);
}

std::vector<const char*> GfxDevice::basicDeviceExts()
{
    return {
        VK_KHR_SWAPCHAIN_EXTENSION_NAME,
        VK_KHR_DYNAMIC_RENDERING_EXTENSION_NAME,
        VK_KHR_ACCELERATION_STRUCTURE_EXTENSION_NAME,
        VK_KHR_RAY_TRACING_PIPELINE_EXTENSION_NAME,
        VK_KHR_RAY_QUERY_EXTENSION_NAME,
        VK_KHR_DEFERRED_HOST_OPERATIONS_EXTENSION_NAME,
        VK_KHR_PUSH_DESCRIPTOR_EXTENSION_NAME,
    };
}

VkPhysicalDeviceFeatures GfxDevice::basicDeviceFeatures()
{
    VkPhysicalDeviceFeatures features{};
    features.samplerAnisotropy = VK_TRUE;
    features.fragmentStoresAndAtomics = VK_TRUE;
    features.independentBlend = VK_TRUE;
    features.shaderInt64 = VK_TRUE;
    return features;
}

} // namespace truvixx
