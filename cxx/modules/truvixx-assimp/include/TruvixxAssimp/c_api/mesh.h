#pragma once

#include "TruvixxAssimp/base_type.h"
#include "TruvixxAssimp/c_api/base.h"
#include "TruvixxAssimp/c_api/truvixx_assimp.export.h"

#ifdef __cplusplus
extern "C" {
#endif

// SOA 布局, 查询-分配-填充模式

/// 获取 mesh 元信息 (用于预分配 buffer)
/// @param scene 场景句柄
/// @param mesh_index mesh 索引
/// @param out [out] 输出 mesh 元信息
/// @return 成功返回 1, 失败返回 0
TRUVIXX_ASSIMP_API ResType truvixx_mesh_get_info(TruvixxSceneHandle scene, uint32_t mesh_index, TruvixxMeshInfo* out);

TRUVIXX_ASSIMP_API ResType truvixx_mesh_fill_positions(TruvixxSceneHandle scene, uint32_t mesh_index, float* out);
TRUVIXX_ASSIMP_API ResType truvixx_mesh_fill_normals(TruvixxSceneHandle scene, uint32_t mesh_index, float* out);
TRUVIXX_ASSIMP_API ResType truvixx_mesh_fill_tangents(TruvixxSceneHandle scene, uint32_t mesh_index, float* out);
TRUVIXX_ASSIMP_API ResType truvixx_mesh_fill_uvs(TruvixxSceneHandle scene, uint32_t mesh_index, float* out);
TRUVIXX_ASSIMP_API ResType truvixx_mesh_fill_indices(TruvixxSceneHandle scene, uint32_t mesh_index, uint32_t* out);

TRUVIXX_ASSIMP_API const TruvixxFloat3* truvixx_mesh_get_positions(TruvixxSceneHandle scene, uint32_t mesh_index);
TRUVIXX_ASSIMP_API const TruvixxFloat3* truvixx_mesh_get_normals(TruvixxSceneHandle scene, uint32_t mesh_index);
TRUVIXX_ASSIMP_API const TruvixxFloat3* truvixx_mesh_get_tangents(TruvixxSceneHandle scene, uint32_t mesh_index);
TRUVIXX_ASSIMP_API const TruvixxFloat2* truvixx_mesh_get_uvs(TruvixxSceneHandle scene, uint32_t mesh_index);
TRUVIXX_ASSIMP_API const uint32_t* truvixx_mesh_get_indices(TruvixxSceneHandle scene, uint32_t mesh_index);

#ifdef __cplusplus
}
#endif
