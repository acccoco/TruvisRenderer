#pragma once

#include <optional>
#include <vector>

#include "vulkan/vulkan.hpp"

namespace truvixx
{

struct GfxDevice;
struct GfxCommandQueue;

/// Swapchain 图像信息
struct GfxSwapchainImageInfo
{
    VkExtent2D image_extent;
    size_t image_count;
    VkFormat image_format;
};

/// Swapchain 封装
///
/// 管理 Vulkan 交换链的创建、销毁和图像获取
struct GfxSwapchain
{
public:
    GfxSwapchain(
        VkPhysicalDevice physical_device,
        GfxDevice* device,
        VkSurfaceKHR surface,
        VkPresentModeKHR present_mode,
        VkSurfaceFormatKHR surface_format,
        VkExtent2D window_extent,
        VkSwapchainKHR old_swapchain = VK_NULL_HANDLE
    );

    ~GfxSwapchain();

    // 禁止拷贝
    GfxSwapchain(const GfxSwapchain&) = delete;
    GfxSwapchain& operator=(const GfxSwapchain&) = delete;

    // 允许移动
    GfxSwapchain(GfxSwapchain&& other) noexcept;
    GfxSwapchain& operator=(GfxSwapchain&& other) noexcept;

    [[nodiscard]] VkSwapchainKHR handle() const { return swapchain_; }
    [[nodiscard]] VkExtent2D extent() const { return extent_; }
    [[nodiscard]] VkFormat format() const { return format_; }
    [[nodiscard]] const std::vector<VkImage>& images() const { return images_; }
    [[nodiscard]] uint32_t current_image_index() const { return current_image_index_; }
    [[nodiscard]] GfxSwapchainImageInfo image_info() const;

    /// 获取下一个可用的交换链图像
    /// @return true 表示需要重建交换链
    bool acquire_next_image(VkSemaphore semaphore, VkFence fence = VK_NULL_HANDLE, uint64_t timeout = UINT64_MAX);

    /// 呈现图像
    /// @return true 表示需要重建交换链
    bool present(VkQueue present_queue, const std::vector<VkSemaphore>& wait_semaphores);

    /// 计算交换链 extent
    static VkExtent2D calculate_swapchain_extent(const VkSurfaceCapabilitiesKHR& capabilities, VkExtent2D window_extent);

private:
    VkSwapchainKHR createSwapchain(
        VkPhysicalDevice physical_device,
        VkSurfaceKHR surface,
        VkFormat format,
        VkColorSpaceKHR color_space,
        VkExtent2D extent,
        VkPresentModeKHR present_mode,
        VkSwapchainKHR old_swapchain
    ) const;

private:
    VkSwapchainKHR swapchain_ = VK_NULL_HANDLE;
    GfxDevice* device_ = nullptr;

    std::vector<VkImage> images_;
    uint32_t current_image_index_ = 0;

    VkFormat format_ = VK_FORMAT_UNDEFINED;
    VkExtent2D extent_{};

    // 扩展函数指针
    PFN_vkCreateSwapchainKHR pfn_vkCreateSwapchain = nullptr;
    PFN_vkDestroySwapchainKHR pfn_vkDestroySwapchain = nullptr;
    PFN_vkGetSwapchainImagesKHR pfn_vkGetSwapchainImages = nullptr;
    PFN_vkAcquireNextImageKHR pfn_vkAcquireNextImage = nullptr;
    PFN_vkQueuePresentKHR pfn_vkQueuePresent = nullptr;
};

} // namespace truvixx
