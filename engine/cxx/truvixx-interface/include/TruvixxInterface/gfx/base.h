#pragma once

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// 结果类型
typedef enum TruvixxGfxResult : uint32_t
{
    TruvixxGfxResultFail = 0,
    TruvixxGfxResultSuccess = 1,
} TruvixxGfxResult;

//=============================================================================
// 不透明句柄类型
//=============================================================================

/// GfxInstance 句柄 (不透明指针)
typedef struct TruvixxGfxInstance* TruvixxGfxInstanceHandle;

/// GfxPhysicalDevice 句柄 (不透明指针)
typedef struct TruvixxGfxPhysicalDevice* TruvixxGfxPhysicalDeviceHandle;

/// GfxDevice 句柄 (不透明指针)
typedef struct TruvixxGfxDevice* TruvixxGfxDeviceHandle;

/// GfxSwapchain 句柄 (不透明指针)
typedef struct TruvixxGfxSwapchain* TruvixxGfxSwapchainHandle;

/// GfxCommandQueue 句柄 (不透明指针)
typedef struct TruvixxGfxCommandQueue* TruvixxGfxCommandQueueHandle;

//=============================================================================
// 数据结构
//=============================================================================

/// Queue Family 信息
typedef struct TruvixxGfxQueueFamily
{
    char name[64];               ///< 队列族名称
    uint32_t queue_family_index; ///< 队列族索引
    uint32_t queue_flags;        ///< VkQueueFlags
    uint32_t queue_count;        ///< 队列数量
} TruvixxGfxQueueFamily;

/// Swapchain 图像信息
typedef struct TruvixxGfxSwapchainImageInfo
{
    uint32_t width;        ///< 图像宽度
    uint32_t height;       ///< 图像高度
    uint32_t image_count;  ///< 图像数量
    uint32_t image_format; ///< VkFormat
} TruvixxGfxSwapchainImageInfo;

/// Device Queue 创建信息
typedef struct TruvixxGfxDeviceQueueCreateInfo
{
    uint32_t queue_family_index; ///< 队列族索引
    uint32_t queue_count;        ///< 队列数量
    const float* priorities;     ///< 队列优先级数组 (元素数量 = queue_count)
} TruvixxGfxDeviceQueueCreateInfo;

/// Present Mode (对应 VkPresentModeKHR)
typedef enum TruvixxGfxPresentMode : uint32_t
{
    TruvixxGfxPresentModeImmediate = 0,
    TruvixxGfxPresentModeMailbox = 1,
    TruvixxGfxPresentModeFifo = 2,
    TruvixxGfxPresentModeFifoRelaxed = 3,
} TruvixxGfxPresentMode;

/// Surface Format (简化版)
typedef struct TruvixxGfxSurfaceFormat
{
    uint32_t format;      ///< VkFormat
    uint32_t color_space; ///< VkColorSpaceKHR
} TruvixxGfxSurfaceFormat;

#ifdef __cplusplus
}
#endif
