#pragma once

#include <optional>
#include <vector>

#include "vulkan/vulkan.hpp"

namespace truvixx {

class GfxDevice;
class GfxCommandQueue;

/// Swapchain 图像信息
struct GfxSwapchainImageInfo {
    VkExtent2D imageExtent;
    size_t imageCount;
    VkFormat imageFormat;
};

/// Swapchain 封装
///
/// 管理 Vulkan 交换链的创建、销毁和图像获取
class GfxSwapchain {
public:
    GfxSwapchain(VkPhysicalDevice physicalDevice,
                 GfxDevice* device,
                 VkSurfaceKHR surface,
                 VkPresentModeKHR presentMode,
                 VkSurfaceFormatKHR surfaceFormat,
                 VkExtent2D windowExtent,
                 VkSwapchainKHR oldSwapchain = VK_NULL_HANDLE);

    ~GfxSwapchain();

    // 禁止拷贝
    GfxSwapchain(const GfxSwapchain&) = delete;
    GfxSwapchain& operator=(const GfxSwapchain&) = delete;

    // 允许移动
    GfxSwapchain(GfxSwapchain&& other) noexcept;
    GfxSwapchain& operator=(GfxSwapchain&& other) noexcept;

    [[nodiscard]] VkSwapchainKHR handle() const { return m_swapchain; }
    [[nodiscard]] VkExtent2D extent() const { return m_extent; }
    [[nodiscard]] VkFormat format() const { return m_format; }
    [[nodiscard]] const std::vector<VkImage>& images() const { return m_images; }
    [[nodiscard]] uint32_t currentImageIndex() const { return m_currentImageIndex; }
    [[nodiscard]] GfxSwapchainImageInfo imageInfo() const;

    /// 获取下一个可用的交换链图像
    /// @return true 表示需要重建交换链
    bool acquireNextImage(VkSemaphore semaphore,
                          VkFence fence = VK_NULL_HANDLE,
                          uint64_t timeout = UINT64_MAX);

    /// 呈现图像
    /// @return true 表示需要重建交换链
    bool present(VkQueue presentQueue, const std::vector<VkSemaphore>& waitSemaphores);

    /// 计算交换链 extent
    static VkExtent2D calculateSwapchainExtent(const VkSurfaceCapabilitiesKHR& capabilities,
                                                VkExtent2D windowExtent);

private:
    VkSwapchainKHR createSwapchain(VkPhysicalDevice physicalDevice,
                                   VkSurfaceKHR surface,
                                   VkFormat format,
                                   VkColorSpaceKHR colorSpace,
                                   VkExtent2D extent,
                                   VkPresentModeKHR presentMode,
                                   VkSwapchainKHR oldSwapchain);

private:
    VkSwapchainKHR m_swapchain = VK_NULL_HANDLE;
    GfxDevice* m_device = nullptr;

    std::vector<VkImage> m_images;
    uint32_t m_currentImageIndex = 0;

    VkFormat m_format = VK_FORMAT_UNDEFINED;
    VkExtent2D m_extent{};

    // 扩展函数指针
    PFN_vkCreateSwapchainKHR m_vkCreateSwapchain = nullptr;
    PFN_vkDestroySwapchainKHR m_vkDestroySwapchain = nullptr;
    PFN_vkGetSwapchainImagesKHR m_vkGetSwapchainImages = nullptr;
    PFN_vkAcquireNextImageKHR m_vkAcquireNextImage = nullptr;
    PFN_vkQueuePresentKHR m_vkQueuePresent = nullptr;
};

} // namespace truvixx
