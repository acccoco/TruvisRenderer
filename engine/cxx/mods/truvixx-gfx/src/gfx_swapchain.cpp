#include "TruvixxGfx/gfx_swapchain.hpp"
#include "TruvixxGfx/gfx_device.hpp"

#include <algorithm>
#include <iostream>
#include <stdexcept>

namespace truvixx
{

GfxSwapchain::GfxSwapchain(VkPhysicalDevice physical_device, GfxDevice* device, VkSurfaceKHR surface, VkPresentModeKHR present_mode, VkSurfaceFormatKHR surface_format, VkExtent2D window_extent, VkSwapchainKHR old_swapchain)
    : device_(device)
{
    VkDevice vkDevice = device->handle();

    // 加载扩展函数
    pfn_vkCreateSwapchain = reinterpret_cast<PFN_vkCreateSwapchainKHR>(
        vkGetDeviceProcAddr(vkDevice, "vkCreateSwapchainKHR")
    );
    pfn_vkDestroySwapchain = reinterpret_cast<PFN_vkDestroySwapchainKHR>(
        vkGetDeviceProcAddr(vkDevice, "vkDestroySwapchainKHR")
    );
    pfn_vkGetSwapchainImages = reinterpret_cast<PFN_vkGetSwapchainImagesKHR>(
        vkGetDeviceProcAddr(vkDevice, "vkGetSwapchainImagesKHR")
    );
    pfn_vkAcquireNextImage = reinterpret_cast<PFN_vkAcquireNextImageKHR>(
        vkGetDeviceProcAddr(vkDevice, "vkAcquireNextImageKHR")
    );
    pfn_vkQueuePresent = reinterpret_cast<PFN_vkQueuePresentKHR>(
        vkGetDeviceProcAddr(vkDevice, "vkQueuePresentKHR")
    );

    // 获取 surface capabilities
    VkSurfaceCapabilitiesKHR capabilities;
    vkGetPhysicalDeviceSurfaceCapabilitiesKHR(physical_device, surface, &capabilities);

    // 计算 extent
    extent_ = calculate_swapchain_extent(capabilities, window_extent);
    format_ = surface_format.format;

    std::cout << "Creating swapchain:\n"
              << "\tSurface current extent: " << capabilities.currentExtent.width << "x" << capabilities.currentExtent.height << "\n"
              << "\tSurface min extent: " << capabilities.minImageExtent.width << "x" << capabilities.minImageExtent.height << "\n"
              << "\tSurface max extent: " << capabilities.maxImageExtent.width << "x" << capabilities.maxImageExtent.height << "\n"
              << "\tWindow extent: " << window_extent.width << "x" << window_extent.height << "\n"
              << "\tFinal swapchain extent: " << extent_.width << "x" << extent_.height << "\n";

    // 创建交换链
    swapchain_ = createSwapchain(
        physical_device,
        surface,
        surface_format.format,
        surface_format.colorSpace,
        extent_,
        present_mode,
        old_swapchain
    );

    // 获取交换链图像
    uint32_t imageCount = 0;
    pfn_vkGetSwapchainImages(vkDevice, swapchain_, &imageCount, nullptr);
    images_.resize(imageCount);
    pfn_vkGetSwapchainImages(vkDevice, swapchain_, &imageCount, images_.data());

    std::cout << "Swapchain created with " << imageCount << " images\n";
}

GfxSwapchain::~GfxSwapchain()
{
    if (swapchain_ != VK_NULL_HANDLE && device_ != nullptr)
    {
        pfn_vkDestroySwapchain(device_->handle(), swapchain_, nullptr);
        swapchain_ = VK_NULL_HANDLE;
    }
}

GfxSwapchain::GfxSwapchain(GfxSwapchain&& other) noexcept
    : swapchain_(other.swapchain_)
    , device_(other.device_)
    , images_(std::move(other.images_))
    , current_image_index_(other.current_image_index_)
    , format_(other.format_)
    , extent_(other.extent_)
    , pfn_vkCreateSwapchain(other.pfn_vkCreateSwapchain)
    , pfn_vkDestroySwapchain(other.pfn_vkDestroySwapchain)
    , pfn_vkGetSwapchainImages(other.pfn_vkGetSwapchainImages)
    , pfn_vkAcquireNextImage(other.pfn_vkAcquireNextImage)
    , pfn_vkQueuePresent(other.pfn_vkQueuePresent)
{
    other.swapchain_ = VK_NULL_HANDLE;
    other.device_ = nullptr;
}

GfxSwapchain& GfxSwapchain::operator=(GfxSwapchain&& other) noexcept
{
    if (this != &other)
    {
        if (swapchain_ != VK_NULL_HANDLE && device_ != nullptr)
        {
            pfn_vkDestroySwapchain(device_->handle(), swapchain_, nullptr);
        }

        swapchain_ = other.swapchain_;
        device_ = other.device_;
        images_ = std::move(other.images_);
        current_image_index_ = other.current_image_index_;
        format_ = other.format_;
        extent_ = other.extent_;
        pfn_vkCreateSwapchain = other.pfn_vkCreateSwapchain;
        pfn_vkDestroySwapchain = other.pfn_vkDestroySwapchain;
        pfn_vkGetSwapchainImages = other.pfn_vkGetSwapchainImages;
        pfn_vkAcquireNextImage = other.pfn_vkAcquireNextImage;
        pfn_vkQueuePresent = other.pfn_vkQueuePresent;

        other.swapchain_ = VK_NULL_HANDLE;
        other.device_ = nullptr;
    }
    return *this;
}

GfxSwapchainImageInfo GfxSwapchain::image_info() const
{
    return {
        .image_extent = extent_,
        .image_count = images_.size(),
        .image_format = format_,
    };
}

bool GfxSwapchain::acquire_next_image(VkSemaphore semaphore, VkFence fence, uint64_t timeout)
{
    VkResult result = pfn_vkAcquireNextImage(
        device_->handle(),
        swapchain_,
        timeout,
        semaphore,
        fence,
        &current_image_index_
    );

    if (result == VK_ERROR_OUT_OF_DATE_KHR)
    {
        std::cout << "Swapchain is out of date when acquiring next image\n";
        return true;
    }

    if (result == VK_SUBOPTIMAL_KHR)
    {
        std::cout << "Swapchain acquire image index " << current_image_index_ << " is not optimal\n";
        return true;
    }

    if (result != VK_SUCCESS)
    {
        throw std::runtime_error("Failed to acquire next swapchain image");
    }

    return false;
}

bool GfxSwapchain::present(VkQueue present_queue, const std::vector<VkSemaphore>& wait_semaphores)
{
    VkPresentInfoKHR presentInfo{};
    presentInfo.sType = VK_STRUCTURE_TYPE_PRESENT_INFO_KHR;
    presentInfo.waitSemaphoreCount = static_cast<uint32_t>(wait_semaphores.size());
    presentInfo.pWaitSemaphores = wait_semaphores.data();
    presentInfo.swapchainCount = 1;
    presentInfo.pSwapchains = &swapchain_;
    presentInfo.pImageIndices = &current_image_index_;

    VkResult result = pfn_vkQueuePresent(present_queue, &presentInfo);

    if (result == VK_ERROR_OUT_OF_DATE_KHR)
    {
        std::cout << "Swapchain is out of date when presenting image\n";
        return true;
    }

    if (result == VK_SUBOPTIMAL_KHR)
    {
        std::cout << "Swapchain present image index " << current_image_index_ << " is not optimal\n";
        return true;
    }

    if (result != VK_SUCCESS)
    {
        throw std::runtime_error("Failed to present swapchain image");
    }

    return false;
}

VkExtent2D GfxSwapchain::calculate_swapchain_extent(
    const VkSurfaceCapabilitiesKHR& capabilities,
    VkExtent2D window_extent
)
{
    if (capabilities.currentExtent.width != 0xFFFFFFFF &&
        capabilities.currentExtent.height != 0xFFFFFFFF)
    {
        return capabilities.currentExtent;
    }

    VkExtent2D extent;
    extent.width = std::clamp(
        window_extent.width,
        capabilities.minImageExtent.width,
        capabilities.maxImageExtent.width
    );
    extent.height = std::clamp(
        window_extent.height,
        capabilities.minImageExtent.height,
        capabilities.maxImageExtent.height
    );

    return extent;
}

VkSwapchainKHR GfxSwapchain::createSwapchain(
    const VkPhysicalDevice physical_device,
    const VkSurfaceKHR surface,
    const VkFormat format,
    const VkColorSpaceKHR color_space,
    const VkExtent2D extent,
    const VkPresentModeKHR present_mode,
    const VkSwapchainKHR old_swapchain
) const
{
    // 获取 surface capabilities
    VkSurfaceCapabilitiesKHR capabilities;
    vkGetPhysicalDeviceSurfaceCapabilitiesKHR(physical_device, surface, &capabilities);

    // 确定 image count
    uint32_t imageCount = capabilities.minImageCount + 1;
    if (capabilities.maxImageCount > 0)
    {
        imageCount = std::min(imageCount, capabilities.maxImageCount);
    }

    VkSwapchainCreateInfoKHR create_info{};
    create_info.sType = VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR;
    create_info.surface = surface;
    create_info.minImageCount = imageCount;
    create_info.imageFormat = format;
    create_info.imageColorSpace = color_space;
    create_info.imageExtent = extent;
    create_info.imageArrayLayers = 1;
    create_info.imageUsage = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    create_info.imageSharingMode = VK_SHARING_MODE_EXCLUSIVE;
    create_info.preTransform = capabilities.currentTransform;
    create_info.compositeAlpha = VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR;
    create_info.presentMode = present_mode;
    create_info.clipped = VK_TRUE;
    create_info.oldSwapchain = old_swapchain;

    VkSwapchainKHR swapchain;
    VkResult result = pfn_vkCreateSwapchain(device_->handle(), &create_info, nullptr, &swapchain);

    if (result != VK_SUCCESS)
    {
        throw std::runtime_error("Failed to create swapchain");
    }

    // 设置调试名称
    device_->set_object_debug_name(swapchain, "main");

    return swapchain;
}

} // namespace truvixx
