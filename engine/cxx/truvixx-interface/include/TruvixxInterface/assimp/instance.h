#pragma once

#include "TruvixxInterface/assimp/base.h"
#include "TruvixxInterface/truvixx_interface.export.h"

#ifdef __cplusplus
extern "C" {
#endif

/// 获取 instance 信息
/// @param scene 场景句柄
/// @param index instance 索引
/// @param out [out] 输出 instance 信息
/// @return 成功返回 1, 失败返回 0
ResType TRUVIXX_INTERFACE_API truvixx_instance_get(TruvixxSceneHandle scene, uint32_t index, TruvixxInstance* out);

/// 获取 instance 引用的 mesh 和材质索引
/// @param scene 场景句柄
/// @param instance_index instance 索引
/// @param out_mesh_indices [out] mesh 索引数组 (大小 >= mesh_count), 可为 NULL
/// @param out_material_indices [out] 材质索引数组 (大小 >= mesh_count), 可为 NULL
/// @return 成功返回 1, 失败返回 0
ResType TRUVIXX_INTERFACE_API truvixx_instance_get_refs(
    TruvixxSceneHandle scene,
    uint32_t instance_index,
    uint32_t* out_mesh_indices,
    uint32_t* out_material_indices
);

#ifdef __cplusplus
}
#endif
