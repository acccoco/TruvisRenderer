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

/// 以下 fill 的 capacity 按输出元素（Float3 / Float2 / uint32_t）计数，不是字节或 float 分量数。
/// 先用 get_info 查询：顶点属性需 vertex_count，索引需 index_count。失败不写出；容量不足返回 Fail。
/// 空数组允许 NULL out 并成功；缺失的可选 normal/tangent 返回 Fail。
/// 调用方负责真实可写容量、对齐及输出不与 scene 存储重叠；scene 必须在整个调用期间存活。
TRUVIXX_ASSIMP_API ResType truvixx_mesh_fill_positions(TruvixxSceneHandle scene, uint32_t mesh_index, TruvixxFloat3* out, uint32_t capacity);
TRUVIXX_ASSIMP_API ResType truvixx_mesh_fill_normals(TruvixxSceneHandle scene, uint32_t mesh_index, TruvixxFloat3* out, uint32_t capacity);
TRUVIXX_ASSIMP_API ResType truvixx_mesh_fill_tangents(TruvixxSceneHandle scene, uint32_t mesh_index, TruvixxFloat3* out, uint32_t capacity);
TRUVIXX_ASSIMP_API ResType truvixx_mesh_fill_uvs(TruvixxSceneHandle scene, uint32_t mesh_index, TruvixxFloat2* out, uint32_t capacity);
TRUVIXX_ASSIMP_API ResType truvixx_mesh_fill_indices(TruvixxSceneHandle scene, uint32_t mesh_index, uint32_t* out, uint32_t capacity);

/// 以下 getter 返回 scene 所有的只读借用，长度见 get_info；空数组、缺失属性或非法输入返回 NULL。
/// 返回数组由 C API 元素类型的实际对象组成，有效至 scene_free；不得写入、释放或与 scene_free 并发使用。
TRUVIXX_ASSIMP_API const TruvixxFloat3* truvixx_mesh_get_positions(TruvixxSceneHandle scene, uint32_t mesh_index);
TRUVIXX_ASSIMP_API const TruvixxFloat3* truvixx_mesh_get_normals(TruvixxSceneHandle scene, uint32_t mesh_index);
TRUVIXX_ASSIMP_API const TruvixxFloat3* truvixx_mesh_get_tangents(TruvixxSceneHandle scene, uint32_t mesh_index);
TRUVIXX_ASSIMP_API const TruvixxFloat2* truvixx_mesh_get_uvs(TruvixxSceneHandle scene, uint32_t mesh_index);
TRUVIXX_ASSIMP_API const uint32_t* truvixx_mesh_get_indices(TruvixxSceneHandle scene, uint32_t mesh_index);

#ifdef __cplusplus
}
#endif
