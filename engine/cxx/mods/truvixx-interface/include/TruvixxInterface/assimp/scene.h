#pragma once

#include "TruvixxInterface/assimp/base.h"
#include "TruvixxInterface/truvixx_interface.export.h"

#ifdef __cplusplus
extern "C" {
#endif

/// 获取 mesh 数量
uint32_t TRUVIXX_INTERFACE_API truvixx_scene_mesh_count(TruvixxSceneHandle scene);

/// 获取材质数量
uint32_t TRUVIXX_INTERFACE_API truvixx_scene_material_count(TruvixxSceneHandle scene);

/// 获取 instance 数量
uint32_t TRUVIXX_INTERFACE_API truvixx_scene_instance_count(TruvixxSceneHandle scene);

#ifdef __cplusplus
}
#endif
