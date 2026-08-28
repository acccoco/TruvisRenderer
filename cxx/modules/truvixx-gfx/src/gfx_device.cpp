#include "TruvixxGfx/gfx_device.hpp"

#include <iostream>
#include <stdexcept>

namespace truvixx
{

GfxDevice::GfxDevice(VkInstance instance, VkPhysicalDevice physical_device, const std::vector<VkDeviceQueueCreateInfo>& queue_create_infos)
{
    // 设备扩展
    auto deviceExts = basic_device_exts();

    std::cout << "Device extensions:\n";
    for (const auto& ext : deviceExts)
    {
        std::cout << "\t" << ext << "\n";
    }

    // 设备特性
    VkPhysicalDeviceFeatures basicFeatures = basic_device_features();

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
    createInfo.queueCreateInfoCount = static_cast<uint32_t>(queue_create_infos.size());
    createInfo.pQueueCreateInfos = queue_create_infos.data();
    createInfo.enabledExtensionCount = static_cast<uint32_t>(deviceExts.size());
    createInfo.ppEnabledExtensionNames = deviceExts.data();
    createInfo.pNext = &allFeatures;

    VkResult result = vkCreateDevice(physical_device, &createInfo, nullptr, &device_);
    if (result != VK_SUCCESS)
    {
        throw std::runtime_error("Failed to create logical device");
    }

    // 加载扩展函数
    pfn_vkSetDebugUtilsObjectName = reinterpret_cast<PFN_vkSetDebugUtilsObjectNameEXT>(
        vkGetDeviceProcAddr(device_, "vkSetDebugUtilsObjectNameEXT")
    );
}

GfxDevice::~GfxDevice()
{
    if (device_ != VK_NULL_HANDLE)
    {
        std::cout << "Destroying GfxDevice\n";
        vkDestroyDevice(device_, nullptr);
        device_ = VK_NULL_HANDLE;
    }
}

GfxDevice::GfxDevice(GfxDevice&& other) noexcept
    : device_(other.device_)
    , pfn_vkSetDebugUtilsObjectName(other.pfn_vkSetDebugUtilsObjectName)
{
    other.device_ = VK_NULL_HANDLE;
    other.pfn_vkSetDebugUtilsObjectName = nullptr;
}

GfxDevice& GfxDevice::operator=(GfxDevice&& other) noexcept
{
    if (this != &other)
    {
        if (device_ != VK_NULL_HANDLE)
        {
            vkDestroyDevice(device_, nullptr);
        }
        device_ = other.device_;
        pfn_vkSetDebugUtilsObjectName = other.pfn_vkSetDebugUtilsObjectName;
        other.device_ = VK_NULL_HANDLE;
        other.pfn_vkSetDebugUtilsObjectName = nullptr;
    }
    return *this;
}

VkQueue GfxDevice::get_queue(uint32_t queueFamilyIndex, uint32_t queueIndex) const
{
    VkQueue queue;
    vkGetDeviceQueue(device_, queueFamilyIndex, queueIndex, &queue);
    return queue;
}

void GfxDevice::wait_idle() const
{
    vkDeviceWaitIdle(device_);
}

void GfxDevice::set_object_debug_name(uint64_t object_handle, VkObjectType object_type, const std::string& name) const
{
    if (pfn_vkSetDebugUtilsObjectName == nullptr)
    {
        return;
    }

    VkDebugUtilsObjectNameInfoEXT nameInfo{};
    nameInfo.sType = VK_STRUCTURE_TYPE_DEBUG_UTILS_OBJECT_NAME_INFO_EXT;
    nameInfo.objectType = object_type;
    nameInfo.objectHandle = object_handle;
    nameInfo.pObjectName = name.c_str();

    pfn_vkSetDebugUtilsObjectName(device_, &nameInfo);
}

template <>
void GfxDevice::set_object_debug_name<VkInstance>(VkInstance handle, const std::string& name) const
{
    set_object_debug_name(reinterpret_cast<uint64_t>(handle), VK_OBJECT_TYPE_INSTANCE, name);
}

template <>
void GfxDevice::set_object_debug_name<VkPhysicalDevice>(VkPhysicalDevice handle, const std::string& name) const
{
    set_object_debug_name(reinterpret_cast<uint64_t>(handle), VK_OBJECT_TYPE_PHYSICAL_DEVICE, name);
}

template <>
void GfxDevice::set_object_debug_name<VkDevice>(VkDevice handle, const std::string& name) const
{
    set_object_debug_name(reinterpret_cast<uint64_t>(handle), VK_OBJECT_TYPE_DEVICE, name);
}

template <>
void GfxDevice::set_object_debug_name<VkQueue>(VkQueue handle, const std::string& name) const
{
    set_object_debug_name(reinterpret_cast<uint64_t>(handle), VK_OBJECT_TYPE_QUEUE, name);
}

template <>
void GfxDevice::set_object_debug_name<VkSwapchainKHR>(VkSwapchainKHR handle, const std::string& name) const
{
    set_object_debug_name(reinterpret_cast<uint64_t>(handle), VK_OBJECT_TYPE_SWAPCHAIN_KHR, name);
}

template <>
void GfxDevice::set_object_debug_name<VkImage>(VkImage handle, const std::string& name) const
{
    set_object_debug_name(reinterpret_cast<uint64_t>(handle), VK_OBJECT_TYPE_IMAGE, name);
}

template <>
void GfxDevice::set_object_debug_name<VkImageView>(VkImageView handle, const std::string& name) const
{
    set_object_debug_name(reinterpret_cast<uint64_t>(handle), VK_OBJECT_TYPE_IMAGE_VIEW, name);
}

std::vector<const char*> GfxDevice::basic_device_exts()
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

VkPhysicalDeviceFeatures GfxDevice::basic_device_features()
{
    VkPhysicalDeviceFeatures features{};
    features.samplerAnisotropy = VK_TRUE;
    features.fragmentStoresAndAtomics = VK_TRUE;
    features.independentBlend = VK_TRUE;
    features.shaderInt64 = VK_TRUE;
    return features;
}

} // namespace truvixx
