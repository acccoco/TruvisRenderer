#include "TruvixxVk/c_api/module.h"

#include <cassert>
#include <type_traits>

/// 自定义函数表必须可按值跨 C ABI 传递；当前 Rust binding 仅支持 Windows x64。
static_assert(sizeof(void*) == 8);
static_assert(std::is_trivial_v<TruvixxVkDeviceDispatch>);
static_assert(std::is_standard_layout_v<TruvixxVkDeviceDispatch>);
static_assert(sizeof(TruvixxVkDeviceDispatch) == 16 && alignof(TruvixxVkDeviceDispatch) == 8);

VkBool32 truvixx_vk_device_init(
    VkDevice device,
    PFN_vkGetDeviceProcAddr get_device_proc_addr,
    TruvixxVkDeviceDispatch* out_dispatch
)
{
    if (!out_dispatch) return VK_FALSE;
    *out_dispatch = {};
    if (device == VK_NULL_HANDLE || !get_device_proc_addr) return VK_FALSE;

    const auto command = reinterpret_cast<PFN_vkCmdPushDescriptorSetKHR>(
        get_device_proc_addr(device, "vkCmdPushDescriptorSetKHR")
    );
    if (!command) return VK_FALSE;

    *out_dispatch = { device, command };
    return VK_TRUE;
}

void truvixx_vk_cmd_push_descriptor_set_khr(
    const TruvixxVkDeviceDispatch* dispatch,
    VkCommandBuffer command_buffer,
    VkPipelineBindPoint pipeline_bind_point,
    VkPipelineLayout layout,
    uint32_t set,
    uint32_t descriptor_write_count,
    const VkWriteDescriptorSet* descriptor_writes
)
{
    assert(dispatch && dispatch->cmd_push_descriptor_set_khr);
    dispatch->cmd_push_descriptor_set_khr(
        command_buffer, pipeline_bind_point, layout, set, descriptor_write_count, descriptor_writes
    );
}
