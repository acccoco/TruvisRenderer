#include "TruvixxGfx/gfx_instance.hpp"

#include <algorithm>
#include <cstring>
#include <functional>
#include <iostream>
#include <set>
#include <stdexcept>

namespace truvixx {

GfxInstance::GfxInstance(const std::string& appName,
                         const std::string& engineName,
                         const std::vector<const char*>& extraInstanceExts)
{
    VkApplicationInfo appInfo{};
    appInfo.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO;
    appInfo.pApplicationName = appName.c_str();
    appInfo.applicationVersion = VK_MAKE_API_VERSION(0, 1, 0, 0);
    appInfo.pEngineName = engineName.c_str();
    appInfo.engineVersion = VK_MAKE_API_VERSION(0, 1, 0, 0);
    appInfo.apiVersion = VK_API_VERSION_1_3;

    auto enabledExtensions = getExtensions(extraInstanceExts);
    auto enabledLayers = getLayers();

    std::cout << "Instance extensions:\n";
    for (const auto& ext : enabledExtensions) {
        std::cout << "\t" << ext << "\n";
    }

    std::cout << "Instance layers:\n";
    for (const auto& layer : enabledLayers) {
        std::cout << "\t" << layer << "\n";
    }

    VkInstanceCreateInfo createInfo{};
    createInfo.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO;
    createInfo.pApplicationInfo = &appInfo;
    createInfo.enabledExtensionCount = static_cast<uint32_t>(enabledExtensions.size());
    createInfo.ppEnabledExtensionNames = enabledExtensions.data();
    createInfo.enabledLayerCount = static_cast<uint32_t>(enabledLayers.size());
    createInfo.ppEnabledLayerNames = enabledLayers.data();

    VkResult result = vkCreateInstance(&createInfo, nullptr, &m_instance);
    if (result != VK_SUCCESS) {
        throw std::runtime_error("Failed to create Vulkan instance");
    }
}

GfxInstance::~GfxInstance()
{
    if (m_instance != VK_NULL_HANDLE) {
        std::cout << "Destroying GfxInstance\n";
        vkDestroyInstance(m_instance, nullptr);
        m_instance = VK_NULL_HANDLE;
    }
}

GfxInstance::GfxInstance(GfxInstance&& other) noexcept
    : m_instance(other.m_instance)
{
    other.m_instance = VK_NULL_HANDLE;
}

GfxInstance& GfxInstance::operator=(GfxInstance&& other) noexcept
{
    if (this != &other) {
        if (m_instance != VK_NULL_HANDLE) {
            vkDestroyInstance(m_instance, nullptr);
        }
        m_instance = other.m_instance;
        other.m_instance = VK_NULL_HANDLE;
    }
    return *this;
}

std::vector<const char*> GfxInstance::getExtensions(const std::vector<const char*>& extraExts) const
{
    uint32_t extensionCount = 0;
    vkEnumerateInstanceExtensionProperties(nullptr, &extensionCount, nullptr);

    std::vector<VkExtensionProperties> availableExtensions(extensionCount);
    vkEnumerateInstanceExtensionProperties(nullptr, &extensionCount, availableExtensions.data());

    std::set<const char*, std::function<bool(const char*, const char*)>> enabledExtensions(
        [](const char* a, const char* b) { return std::strcmp(a, b) < 0; }
    );

    auto enableExt = [&](const char* extName) {
        bool supported = std::any_of(
            availableExtensions.begin(),
            availableExtensions.end(),
            [extName](const VkExtensionProperties& props) {
                return std::strcmp(extName, props.extensionName) == 0;
            }
        );

        if (supported) {
            enabledExtensions.insert(extName);
        } else {
            throw std::runtime_error(std::string("Required instance extension is missing: ") + extName);
        }
    };

    // 启用基础 extensions
    for (const auto& ext : basicInstanceExts()) {
        enableExt(ext);
    }

    // 启用额外的 extensions
    for (const auto& ext : extraExts) {
        enableExt(ext);
    }

    return {enabledExtensions.begin(), enabledExtensions.end()};
}

std::vector<const char*> GfxInstance::getLayers() const
{
    uint32_t layerCount = 0;
    vkEnumerateInstanceLayerProperties(&layerCount, nullptr);

    std::vector<VkLayerProperties> availableLayers(layerCount);
    vkEnumerateInstanceLayerProperties(&layerCount, availableLayers.data());

    std::vector<const char*> enabledLayers;

    auto enableLayer = [&](const char* layerName) {
        bool supported = std::any_of(
            availableLayers.begin(),
            availableLayers.end(),
            [layerName](const VkLayerProperties& props) {
                return std::strcmp(layerName, props.layerName) == 0;
            }
        );

        if (supported) {
            enabledLayers.push_back(layerName);
        } else {
            throw std::runtime_error(std::string("Required instance layer is missing: ") + layerName);
        }
    };

    for (const auto& layer : basicInstanceLayers()) {
        enableLayer(layer);
    }

    return enabledLayers;
}

std::vector<const char*> GfxInstance::basicInstanceExts()
{
    return {
        VK_EXT_DEBUG_UTILS_EXTENSION_NAME,
    };
}

std::vector<const char*> GfxInstance::basicInstanceLayers()
{
    // 无需开启 validation layer，使用 vulkan configurator 控制 validation layer 的开启
    return {};
}

} // namespace truvixx
