#pragma once

#include "TruvixxInterface/gfx/base.h"
#include "TruvixxInterface/truvixx_interface.export.h"

#ifdef __cplusplus
extern "C" {
#endif

/// 创建 Swapchain
/// @param vk_physical_device VkPhysicalDevice 句柄 (作为 uint64_t)
/// @param device GfxDevice 句柄
/// @param vk_surface VkSurfaceKHR 句柄 (作为 uint64_t)
/// @param present_mode 呈现模式
/// @param surface_format 表面格式
/// @param width 窗口宽度
/// @param height 窗口高度
/// @param old_swapchain 旧的 VkSwapchainKHR (可以为 0)
/// @return Swapchain 句柄，失败返回 NULL
TruvixxGfxSwapchainHandle TRUVIXX_INTERFACE_API truvixx_gfx_swapchain_create(
    uint64_t vk_physical_device,
    TruvixxGfxDeviceHandle device,
    uint64_t vk_surface,
    TruvixxGfxPresentMode present_mode,
    TruvixxGfxSurfaceFormat surface_format,
    uint32_t width,
    uint32_t height,
    uint64_t old_swapchain
);

/// 销毁 Swapchain
/// @param swapchain Swapchain 句柄 (可以为 NULL)
void TRUVIXX_INTERFACE_API truvixx_gfx_swapchain_destroy(TruvixxGfxSwapchainHandle swapchain);

/// 获取 VkSwapchainKHR 句柄
/// @param swapchain Swapchain 句柄
/// @return VkSwapchainKHR (作为 uint64_t)，失败返回 0
uint64_t TRUVIXX_INTERFACE_API truvixx_gfx_swapchain_handle(TruvixxGfxSwapchainHandle swapchain);

/// 获取 Swapchain 宽度
/// @param swapchain Swapchain 句柄
/// @return 宽度
uint32_t TRUVIXX_INTERFACE_API truvixx_gfx_swapchain_extent_width(TruvixxGfxSwapchainHandle swapchain);

/// 获取 Swapchain 高度
/// @param swapchain Swapchain 句柄
/// @return 高度
uint32_t TRUVIXX_INTERFACE_API truvixx_gfx_swapchain_extent_height(TruvixxGfxSwapchainHandle swapchain);

/// 获取 Swapchain 图像格式
/// @param swapchain Swapchain 句柄
/// @return VkFormat (作为 uint32_t)
uint32_t TRUVIXX_INTERFACE_API truvixx_gfx_swapchain_format(TruvixxGfxSwapchainHandle swapchain);

/// 获取 Swapchain 图像数量
/// @param swapchain Swapchain 句柄
/// @return 图像数量
uint32_t TRUVIXX_INTERFACE_API truvixx_gfx_swapchain_image_count(TruvixxGfxSwapchainHandle swapchain);

/// 获取 Swapchain 图像
/// @param swapchain Swapchain 句柄
/// @param out_images 输出 VkImage 数组 (每个元素为 uint64_t)
/// @param count 数组长度 (需要先调用 truvixx_gfx_swapchain_image_count 获取)
void TRUVIXX_INTERFACE_API truvixx_gfx_swapchain_get_images(
    TruvixxGfxSwapchainHandle swapchain,
    uint64_t* out_images,
    uint32_t count
);

/// 获取当前图像索引
/// @param swapchain Swapchain 句柄
/// @return 当前图像索引
uint32_t TRUVIXX_INTERFACE_API truvixx_gfx_swapchain_current_image_index(TruvixxGfxSwapchainHandle swapchain);

/// 获取 Swapchain 图像信息
/// @param swapchain Swapchain 句柄
/// @param out 输出图像信息
void TRUVIXX_INTERFACE_API truvixx_gfx_swapchain_image_info(
    TruvixxGfxSwapchainHandle swapchain,
    TruvixxGfxSwapchainImageInfo* out
);

/// 获取下一个可用的 Swapchain 图像
/// @param swapchain Swapchain 句柄
/// @param vk_semaphore VkSemaphore 句柄 (作为 uint64_t)
/// @param vk_fence VkFence 句柄 (作为 uint64_t, 可以为 0)
/// @param timeout 超时时间 (纳秒)
/// @return 1 表示需要重建 Swapchain，0 表示成功
uint32_t TRUVIXX_INTERFACE_API truvixx_gfx_swapchain_acquire_next_image(
    TruvixxGfxSwapchainHandle swapchain,
    uint64_t vk_semaphore,
    uint64_t vk_fence,
    uint64_t timeout
);

/// 呈现图像
/// @param swapchain Swapchain 句柄
/// @param vk_queue VkQueue 句柄 (作为 uint64_t)
/// @param wait_semaphores 等待的 VkSemaphore 数组 (每个元素为 uint64_t)
/// @param semaphore_count 信号量数量
/// @return 1 表示需要重建 Swapchain，0 表示成功
uint32_t TRUVIXX_INTERFACE_API truvixx_gfx_swapchain_present(
    TruvixxGfxSwapchainHandle swapchain,
    uint64_t vk_queue,
    const uint64_t* wait_semaphores,
    uint32_t semaphore_count
);

#ifdef __cplusplus
}
#endif
