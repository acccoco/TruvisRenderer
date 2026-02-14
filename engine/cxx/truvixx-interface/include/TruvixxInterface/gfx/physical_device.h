#pragma once

#include "TruvixxInterface/gfx/base.h"
#include "TruvixxInterface/truvixx_interface.export.h"

#ifdef __cplusplus
extern "C" {
#endif

/// 创建物理设备 (自动选择最佳 GPU)
/// @param vk_instance VkInstance 句柄 (作为 uint64_t)
/// @return PhysicalDevice 句柄，失败返回 NULL
TruvixxGfxPhysicalDeviceHandle TRUVIXX_INTERFACE_API truvixx_gfx_physical_device_create(
    uint64_t vk_instance
);

/// 销毁物理设备
/// @param pdevice PhysicalDevice 句柄 (可以为 NULL)
void TRUVIXX_INTERFACE_API truvixx_gfx_physical_device_destroy(TruvixxGfxPhysicalDeviceHandle pdevice);

/// 获取 VkPhysicalDevice 句柄
/// @param pdevice PhysicalDevice 句柄
/// @return VkPhysicalDevice (作为 uint64_t)，失败返回 0
uint64_t TRUVIXX_INTERFACE_API truvixx_gfx_physical_device_handle(TruvixxGfxPhysicalDeviceHandle pdevice);

/// 获取图形队列族
/// @param pdevice PhysicalDevice 句柄
/// @param out 输出队列族信息
/// @return 成功返回 TruvixxGfxResultSuccess
TruvixxGfxResult TRUVIXX_INTERFACE_API truvixx_gfx_physical_device_gfx_queue_family(
    TruvixxGfxPhysicalDeviceHandle pdevice,
    TruvixxGfxQueueFamily* out
);

/// 检查是否有独立的计算队列族
/// @param pdevice PhysicalDevice 句柄
/// @return 1 表示有，0 表示没有
uint32_t TRUVIXX_INTERFACE_API truvixx_gfx_physical_device_has_compute_queue_family(
    TruvixxGfxPhysicalDeviceHandle pdevice
);

/// 获取计算队列族 (需先调用 has_compute_queue_family 确认存在)
/// @param pdevice PhysicalDevice 句柄
/// @param out 输出队列族信息
/// @return 成功返回 TruvixxGfxResultSuccess
TruvixxGfxResult TRUVIXX_INTERFACE_API truvixx_gfx_physical_device_compute_queue_family(
    TruvixxGfxPhysicalDeviceHandle pdevice,
    TruvixxGfxQueueFamily* out
);

/// 检查是否有独立的传输队列族
/// @param pdevice PhysicalDevice 句柄
/// @return 1 表示有，0 表示没有
uint32_t TRUVIXX_INTERFACE_API truvixx_gfx_physical_device_has_transfer_queue_family(
    TruvixxGfxPhysicalDeviceHandle pdevice
);

/// 获取传输队列族 (需先调用 has_transfer_queue_family 确认存在)
/// @param pdevice PhysicalDevice 句柄
/// @param out 输出队列族信息
/// @return 成功返回 TruvixxGfxResultSuccess
TruvixxGfxResult TRUVIXX_INTERFACE_API truvixx_gfx_physical_device_transfer_queue_family(
    TruvixxGfxPhysicalDeviceHandle pdevice,
    TruvixxGfxQueueFamily* out
);

/// 检查是否为独立显卡
/// @param pdevice PhysicalDevice 句柄
/// @return 1 表示是独立显卡，0 表示不是
uint32_t TRUVIXX_INTERFACE_API truvixx_gfx_physical_device_is_discrete_gpu(
    TruvixxGfxPhysicalDeviceHandle pdevice
);

/// 获取设备名称
/// @param pdevice PhysicalDevice 句柄
/// @param out_name 输出缓冲区 (至少 256 字节)
/// @param buffer_size 缓冲区大小
void TRUVIXX_INTERFACE_API truvixx_gfx_physical_device_name(
    TruvixxGfxPhysicalDeviceHandle pdevice,
    char* out_name,
    uint32_t buffer_size
);

#ifdef __cplusplus
}
#endif
