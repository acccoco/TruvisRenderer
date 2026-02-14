#pragma once

#include "TruvixxInterface/gfx/base.h"
#include "TruvixxInterface/truvixx_interface.export.h"

#ifdef __cplusplus
extern "C" {
#endif

/// 创建逻辑设备
/// @param vk_instance VkInstance 句柄 (作为 uint64_t)
/// @param vk_physical_device VkPhysicalDevice 句柄 (作为 uint64_t)
/// @param queue_infos 队列创建信息数组
/// @param queue_info_count 队列创建信息数量
/// @return Device 句柄，失败返回 NULL
TruvixxGfxDeviceHandle TRUVIXX_INTERFACE_API truvixx_gfx_device_create(
    uint64_t vk_instance,
    uint64_t vk_physical_device,
    const TruvixxGfxDeviceQueueCreateInfo* queue_infos,
    uint32_t queue_info_count
);

/// 销毁逻辑设备
/// @param device Device 句柄 (可以为 NULL)
void TRUVIXX_INTERFACE_API truvixx_gfx_device_destroy(TruvixxGfxDeviceHandle device);

/// 获取 VkDevice 句柄
/// @param device Device 句柄
/// @return VkDevice (作为 uint64_t)，失败返回 0
uint64_t TRUVIXX_INTERFACE_API truvixx_gfx_device_handle(TruvixxGfxDeviceHandle device);

/// 获取指定队列族和索引的队列
/// @param device Device 句柄
/// @param queue_family_index 队列族索引
/// @param queue_index 队列索引
/// @return VkQueue (作为 uint64_t)，失败返回 0
uint64_t TRUVIXX_INTERFACE_API truvixx_gfx_device_get_queue(
    TruvixxGfxDeviceHandle device,
    uint32_t queue_family_index,
    uint32_t queue_index
);

/// 等待设备空闲
/// @param device Device 句柄
void TRUVIXX_INTERFACE_API truvixx_gfx_device_wait_idle(TruvixxGfxDeviceHandle device);

/// 设置 Vulkan 对象的调试名称
/// @param device Device 句柄
/// @param object_handle Vulkan 对象句柄 (作为 uint64_t)
/// @param object_type VkObjectType (作为 uint32_t)
/// @param name 调试名称 (UTF-8)
void TRUVIXX_INTERFACE_API truvixx_gfx_device_set_object_debug_name(
    TruvixxGfxDeviceHandle device,
    uint64_t object_handle,
    uint32_t object_type,
    const char* name
);

#ifdef __cplusplus
}
#endif
