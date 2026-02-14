#pragma once

#include "TruvixxInterface/gfx/base.h"
#include "TruvixxInterface/truvixx_interface.export.h"

#ifdef __cplusplus
extern "C" {
#endif

/// 创建 CommandQueue
/// @param vk_queue VkQueue 句柄 (作为 uint64_t)
/// @param queue_family 队列族信息
/// @param device GfxDevice 句柄
/// @return CommandQueue 句柄，失败返回 NULL
TruvixxGfxCommandQueueHandle TRUVIXX_INTERFACE_API truvixx_gfx_command_queue_create(
    uint64_t vk_queue,
    const TruvixxGfxQueueFamily* queue_family,
    TruvixxGfxDeviceHandle device
);

/// 销毁 CommandQueue
/// @param queue CommandQueue 句柄 (可以为 NULL)
void TRUVIXX_INTERFACE_API truvixx_gfx_command_queue_destroy(TruvixxGfxCommandQueueHandle queue);

/// 获取 VkQueue 句柄
/// @param queue CommandQueue 句柄
/// @return VkQueue (作为 uint64_t)，失败返回 0
uint64_t TRUVIXX_INTERFACE_API truvixx_gfx_command_queue_handle(TruvixxGfxCommandQueueHandle queue);

/// 获取队列族信息
/// @param queue CommandQueue 句柄
/// @param out 输出队列族信息
void TRUVIXX_INTERFACE_API truvixx_gfx_command_queue_queue_family(
    TruvixxGfxCommandQueueHandle queue,
    TruvixxGfxQueueFamily* out
);

/// 等待队列空闲
/// @param queue CommandQueue 句柄
void TRUVIXX_INTERFACE_API truvixx_gfx_command_queue_wait_idle(TruvixxGfxCommandQueueHandle queue);

/// 提交命令缓冲区 (VkSubmitInfo2)
/// @param queue CommandQueue 句柄
/// @param submit_infos VkSubmitInfo2 数组指针
/// @param submit_count 提交信息数量
/// @param vk_fence VkFence 句柄 (作为 uint64_t, 可以为 0)
void TRUVIXX_INTERFACE_API truvixx_gfx_command_queue_submit(
    TruvixxGfxCommandQueueHandle queue,
    const void* submit_infos,
    uint32_t submit_count,
    uint64_t vk_fence
);

/// 开始调试标签
/// @param queue CommandQueue 句柄
/// @param label_name 标签名称 (UTF-8)
/// @param r 颜色 R 分量 (0.0 - 1.0)
/// @param g 颜色 G 分量 (0.0 - 1.0)
/// @param b 颜色 B 分量 (0.0 - 1.0)
/// @param a 颜色 A 分量 (0.0 - 1.0)
void TRUVIXX_INTERFACE_API truvixx_gfx_command_queue_begin_label(
    TruvixxGfxCommandQueueHandle queue,
    const char* label_name,
    float r,
    float g,
    float b,
    float a
);

/// 结束调试标签
/// @param queue CommandQueue 句柄
void TRUVIXX_INTERFACE_API truvixx_gfx_command_queue_end_label(TruvixxGfxCommandQueueHandle queue);

/// 插入调试标签
/// @param queue CommandQueue 句柄
/// @param label_name 标签名称 (UTF-8)
/// @param r 颜色 R 分量 (0.0 - 1.0)
/// @param g 颜色 G 分量 (0.0 - 1.0)
/// @param b 颜色 B 分量 (0.0 - 1.0)
/// @param a 颜色 A 分量 (0.0 - 1.0)
void TRUVIXX_INTERFACE_API truvixx_gfx_command_queue_insert_label(
    TruvixxGfxCommandQueueHandle queue,
    const char* label_name,
    float r,
    float g,
    float b,
    float a
);

#ifdef __cplusplus
}
#endif
