#include "TruvixxGfx/gfx_swapchain.hpp"
#include "TruvixxGfx/gfx_device.hpp"

#include <algorithm>
#include <iostream>
#include <stdexcept>

namespace truvixx {

GfxSwapchain::GfxSwapchain(VkPhysicalDevice physicalDevice,
                           GfxDevice* device,
                           VkSurfaceKHR surface,
                           VkPresentModeKHR presentMode,
                           VkSurfaceFormatKHR surfaceFormat,
                           VkExtent2D windowExtent,
                           VkSwapchainKHR oldSwapchain)
    : m_device(device)
{
    VkDevice vkDevice = device->handle();

    // 加载扩展函数
    m_vkCreateSwapchain = reinterpret_cast<PFN_vkCreateSwapchainKHR>(
        vkGetDeviceProcAddr(vkDevice, "vkCreateSwapchainKHR"));
    m_vkDestroySwapchain = reinterpret_cast<PFN_vkDestroySwapchainKHR>(
        vkGetDeviceProcAddr(vkDevice, "vkDestroySwapchainKHR"));
    m_vkGetSwapchainImages = reinterpret_cast<PFN_vkGetSwapchainImagesKHR>(
        vkGetDeviceProcAddr(vkDevice, "vkGetSwapchainImagesKHR"));
    m_vkAcquireNextImage = reinterpret_cast<PFN_vkAcquireNextImageKHR>(
        vkGetDeviceProcAddr(vkDevice, "vkAcquireNextImageKHR"));
    m_vkQueuePresent = reinterpret_cast<PFN_vkQueuePresentKHR>(
        vkGetDeviceProcAddr(vkDevice, "vkQueuePresentKHR"));

    // 获取 surface capabilities
    VkSurfaceCapabilitiesKHR capabilities;
    vkGetPhysicalDeviceSurfaceCapabilitiesKHR(physicalDevice, surface, &capabilities);

    // 计算 extent
    m_extent = calculateSwapchainExtent(capabilities, windowExtent);
    m_format = surfaceFormat.format;

    std::cout << "Creating swapchain:\n"
              << "\tSurface current extent: " << capabilities.currentExtent.width << "x" << capabilities.currentExtent.height << "\n"
              << "\tSurface min extent: " << capabilities.minImageExtent.width << "x" << capabilities.minImageExtent.height << "\n"
              << "\tSurface max extent: " << capabilities.maxImageExtent.width << "x" << capabilities.maxImageExtent.height << "\n"
              << "\tWindow extent: " << windowExtent.width << "x" << windowExtent.height << "\n"
              << "\tFinal swapchain extent: " << m_extent.width << "x" << m_extent.height << "\n";

    // 创建交换链
    m_swapchain = createSwapchain(
        physicalDevice,
        surface,
        surfaceFormat.format,
        surfaceFormat.colorSpace,
        m_extent,
        presentMode,
        oldSwapchain);

    // 获取交换链图像
    uint32_t imageCount = 0;
    m_vkGetSwapchainImages(vkDevice, m_swapchain, &imageCount, nullptr);
    m_images.resize(imageCount);
    m_vkGetSwapchainImages(vkDevice, m_swapchain, &imageCount, m_images.data());

    std::cout << "Swapchain created with " << imageCount << " images\n";
}

GfxSwapchain::~GfxSwapchain()
{
    if (m_swapchain != VK_NULL_HANDLE && m_device != nullptr) {
        m_vkDestroySwapchain(m_device->handle(), m_swapchain, nullptr);
        m_swapchain = VK_NULL_HANDLE;
    }
}

GfxSwapchain::GfxSwapchain(GfxSwapchain&& other) noexcept
    : m_swapchain(other.m_swapchain)
    , m_device(other.m_device)
    , m_images(std::move(other.m_images))
    , m_currentImageIndex(other.m_currentImageIndex)
    , m_format(other.m_format)
    , m_extent(other.m_extent)
    , m_vkCreateSwapchain(other.m_vkCreateSwapchain)
    , m_vkDestroySwapchain(other.m_vkDestroySwapchain)
    , m_vkGetSwapchainImages(other.m_vkGetSwapchainImages)
    , m_vkAcquireNextImage(other.m_vkAcquireNextImage)
    , m_vkQueuePresent(other.m_vkQueuePresent)
{
    other.m_swapchain = VK_NULL_HANDLE;
    other.m_device = nullptr;
}

GfxSwapchain& GfxSwapchain::operator=(GfxSwapchain&& other) noexcept
{
    if (this != &other) {
        if (m_swapchain != VK_NULL_HANDLE && m_device != nullptr) {
            m_vkDestroySwapchain(m_device->handle(), m_swapchain, nullptr);
        }

        m_swapchain = other.m_swapchain;
        m_device = other.m_device;
        m_images = std::move(other.m_images);
        m_currentImageIndex = other.m_currentImageIndex;
        m_format = other.m_format;
        m_extent = other.m_extent;
        m_vkCreateSwapchain = other.m_vkCreateSwapchain;
        m_vkDestroySwapchain = other.m_vkDestroySwapchain;
        m_vkGetSwapchainImages = other.m_vkGetSwapchainImages;
        m_vkAcquireNextImage = other.m_vkAcquireNextImage;
        m_vkQueuePresent = other.m_vkQueuePresent;

        other.m_swapchain = VK_NULL_HANDLE;
        other.m_device = nullptr;
    }
    return *this;
}

GfxSwapchainImageInfo GfxSwapchain::imageInfo() const
{
    return {
        .imageExtent = m_extent,
        .imageCount = m_images.size(),
        .imageFormat = m_format,
    };
}

bool GfxSwapchain::acquireNextImage(VkSemaphore semaphore, VkFence fence, uint64_t timeout)
{
    VkResult result = m_vkAcquireNextImage(
        m_device->handle(),
        m_swapchain,
        timeout,
        semaphore,
        fence,
        &m_currentImageIndex);

    if (result == VK_ERROR_OUT_OF_DATE_KHR) {
        std::cout << "Swapchain is out of date when acquiring next image\n";
        return true;
    }

    if (result == VK_SUBOPTIMAL_KHR) {
        std::cout << "Swapchain acquire image index " << m_currentImageIndex << " is not optimal\n";
        return true;
    }

    if (result != VK_SUCCESS) {
        throw std::runtime_error("Failed to acquire next swapchain image");
    }

    return false;
}

bool GfxSwapchain::present(VkQueue presentQueue, const std::vector<VkSemaphore>& waitSemaphores)
{
    VkPresentInfoKHR presentInfo{};
    presentInfo.sType = VK_STRUCTURE_TYPE_PRESENT_INFO_KHR;
    presentInfo.waitSemaphoreCount = static_cast<uint32_t>(waitSemaphores.size());
    presentInfo.pWaitSemaphores = waitSemaphores.data();
    presentInfo.swapchainCount = 1;
    presentInfo.pSwapchains = &m_swapchain;
    presentInfo.pImageIndices = &m_currentImageIndex;

    VkResult result = m_vkQueuePresent(presentQueue, &presentInfo);

    if (result == VK_ERROR_OUT_OF_DATE_KHR) {
        std::cout << "Swapchain is out of date when presenting image\n";
        return true;
    }

    if (result == VK_SUBOPTIMAL_KHR) {
        std::cout << "Swapchain present image index " << m_currentImageIndex << " is not optimal\n";
        return true;
    }

    if (result != VK_SUCCESS) {
        throw std::runtime_error("Failed to present swapchain image");
    }

    return false;
}

VkExtent2D GfxSwapchain::calculateSwapchainExtent(const VkSurfaceCapabilitiesKHR& capabilities,
                                                  VkExtent2D windowExtent)
{
    if (capabilities.currentExtent.width != 0xFFFFFFFF &&
        capabilities.currentExtent.height != 0xFFFFFFFF) {
        return capabilities.currentExtent;
    }

    VkExtent2D extent;
    extent.width = std::clamp(
        windowExtent.width,
        capabilities.minImageExtent.width,
        capabilities.maxImageExtent.width);
    extent.height = std::clamp(
        windowExtent.height,
        capabilities.minImageExtent.height,
        capabilities.maxImageExtent.height);

    return extent;
}

VkSwapchainKHR GfxSwapchain::createSwapchain(VkPhysicalDevice physicalDevice,
                                              VkSurfaceKHR surface,
                                              VkFormat format,
                                              VkColorSpaceKHR colorSpace,
                                              VkExtent2D extent,
                                              VkPresentModeKHR presentMode,
                                              VkSwapchainKHR oldSwapchain)
{
    // 获取 surface capabilities
    VkSurfaceCapabilitiesKHR capabilities;
    vkGetPhysicalDeviceSurfaceCapabilitiesKHR(physicalDevice, surface, &capabilities);

    // 确定 image count
    uint32_t imageCount = capabilities.minImageCount + 1;
    if (capabilities.maxImageCount > 0) {
        imageCount = std::min(imageCount, capabilities.maxImageCount);
    }

    VkSwapchainCreateInfoKHR createInfo{};
    createInfo.sType = VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR;
    createInfo.surface = surface;
    createInfo.minImageCount = imageCount;
    createInfo.imageFormat = format;
    createInfo.imageColorSpace = colorSpace;
    createInfo.imageExtent = extent;
    createInfo.imageArrayLayers = 1;
    createInfo.imageUsage = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    createInfo.imageSharingMode = VK_SHARING_MODE_EXCLUSIVE;
    createInfo.preTransform = capabilities.currentTransform;
    createInfo.compositeAlpha = VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR;
    createInfo.presentMode = presentMode;
    createInfo.clipped = VK_TRUE;
    createInfo.oldSwapchain = oldSwapchain;

    VkSwapchainKHR swapchain;
    VkResult result = m_vkCreateSwapchain(m_device->handle(), &createInfo, nullptr, &swapchain);

    if (result != VK_SUCCESS) {
        throw std::runtime_error("Failed to create swapchain");
    }

    // 设置调试名称
    m_device->setObjectDebugName(swapchain, "main");

    return swapchain;
}

} // namespace truvixx
