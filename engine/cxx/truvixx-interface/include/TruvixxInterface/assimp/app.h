#pragma once

#include "TruvixxInterface/assimp/base.h"
#include "TruvixxInterface/truvixx_interface.export.h"

#ifdef __cplusplus
extern "C" {
#endif

/// 加载场景文件
/// @param path 文件路径 (UTF-8)
/// @return 场景句柄, 失败返回 NULL
TruvixxSceneHandle TRUVIXX_INTERFACE_API truvixx_scene_load(const char* path);

/// 释放场景
/// @param scene 场景句柄 (可以为 NULL)
void TRUVIXX_INTERFACE_API truvixx_scene_free(TruvixxSceneHandle scene);

#ifdef __cplusplus
}
#endif
