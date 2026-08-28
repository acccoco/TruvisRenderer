#pragma once

#include "TruvixxAssimp/c_api/base.h"
#include "TruvixxAssimp/c_api/truvixx_assimp.export.h"

#ifdef __cplusplus
extern "C" {
#endif

/// 获取 mesh 数量
uint32_t TRUVIXX_ASSIMP_API truvixx_scene_mesh_count(TruvixxSceneHandle scene);

/// 获取材质数量
uint32_t TRUVIXX_ASSIMP_API truvixx_scene_material_count(TruvixxSceneHandle scene);

/// 获取 instance 数量
uint32_t TRUVIXX_ASSIMP_API truvixx_scene_instance_count(TruvixxSceneHandle scene);

#ifdef __cplusplus
}
#endif
