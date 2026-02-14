#pragma once

#include "TruvixxInterface/gfx/base.h"
#include "TruvixxInterface/truvixx_interface.export.h"

#ifdef __cplusplus
extern "C" {
#endif

/// 创建 Vulkan Instance
/// @param app_name 应用名称 (UTF-8)
/// @param engine_name 引擎名称 (UTF-8)
/// @param extra_exts 额外的 instance extensions (可以为 NULL)
/// @param ext_count extra_exts 数组长度
/// @return Instance 句柄，失败返回 NULL
TruvixxGfxInstanceHandle TRUVIXX_INTERFACE_API truvixx_gfx_instance_create(
    const char* app_name,
    const char* engine_name,
    const char** extra_exts,
    uint32_t ext_count
);

/// 销毁 Vulkan Instance
/// @param instance Instance 句柄 (可以为 NULL)
void TRUVIXX_INTERFACE_API truvixx_gfx_instance_destroy(TruvixxGfxInstanceHandle instance);

/// 获取 VkInstance 句柄
/// @param instance Instance 句柄
/// @return VkInstance (作为 uint64_t)，失败返回 0
uint64_t TRUVIXX_INTERFACE_API truvixx_gfx_instance_handle(TruvixxGfxInstanceHandle instance);

#ifdef __cplusplus
}
#endif
