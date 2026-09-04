#pragma once

#ifndef VK_NO_PROTOTYPES
#define VK_NO_PROTOTYPES
#endif
#include <vulkan/vulkan_core.h>

#include "TruvixxVk/c_api/truvixx_vk.export.h"

#ifdef __cplusplus
extern "C" {
#endif

/// 由调用方按值持有的设备函数表，不拥有 Vulkan 对象或 DLL。
/// 函数指针只能用于该 device 的子对象；所有调用结束前必须保持原 ash loader 链路有效。
typedef struct TruvixxVkDeviceDispatch
{
    VkDevice device;
    PFN_vkCmdPushDescriptorSetKHR cmd_push_descriptor_set_khr;
} TruvixxVkDeviceDispatch;

/// 从 ash 所属 Instance 提供的原生入口查询命令，不加载 DLL、不创建 Vulkan 对象。
/// out_dispatch 必须指向可写结构；失败时保持清零，成功后才可调用命令接口。
TRUVIXX_VK_API VkBool32 truvixx_vk_device_init(
    VkDevice device,
    PFN_vkGetDeviceProcAddr get_device_proc_addr,
    TruvixxVkDeviceDispatch* out_dispatch
);

/// 同步透传所有 descriptor 指针和 pNext，不复制、不保存、不延迟执行。
/// dispatch 必须已初始化；command_buffer 与 layout 必须属于同一 device，并满足 Vulkan 录制与同步规则。
TRUVIXX_VK_API void truvixx_vk_cmd_push_descriptor_set_khr(
    const TruvixxVkDeviceDispatch* dispatch,
    VkCommandBuffer command_buffer,
    VkPipelineBindPoint pipeline_bind_point,
    VkPipelineLayout layout,
    uint32_t set,
    uint32_t descriptor_write_count,
    const VkWriteDescriptorSet* descriptor_writes
);

#ifdef __cplusplus
}
#endif
