#pragma once

#include "TruvixxAssimp/c_api/base.h"
#include "TruvixxAssimp/c_api/truvixx_assimp.export.h"

#ifdef __cplusplus
extern "C" {
#endif

/// 加载场景文件。
///
/// 加载失败时仍可能返回非空句柄，以便调用方读取 importer 错误信息。
/// 调用方必须通过 `truvixx_scene_is_loaded` 判断导入是否成功，并最终调用
/// `truvixx_scene_free` 释放非空句柄。
/// @param path 文件路径 (UTF-8)
/// @return 场景句柄；参数非法或无法创建句柄时返回 NULL
TruvixxSceneHandle TRUVIXX_ASSIMP_API truvixx_scene_load(const char* path);

/// 查询场景是否已成功导入。
/// @param scene 场景句柄
/// @return 成功导入返回 ResTypeSuccess，否则返回 ResTypeFail
ResType TRUVIXX_ASSIMP_API truvixx_scene_is_loaded(TruvixxSceneHandle scene);

/// 读取最近一次导入失败的错误信息。
///
/// 返回指针由 scene 持有，直到下一次加载或 `truvixx_scene_free` 前有效。
/// @param scene 场景句柄
/// @return UTF-8 错误字符串；无错误或 scene 为空时返回空字符串
TRUVIXX_ASSIMP_API const char* truvixx_scene_last_error(TruvixxSceneHandle scene);

/// 释放场景
/// @param scene 场景句柄 (可以为 NULL)
void TRUVIXX_ASSIMP_API truvixx_scene_free(TruvixxSceneHandle scene);

#ifdef __cplusplus
}
#endif
