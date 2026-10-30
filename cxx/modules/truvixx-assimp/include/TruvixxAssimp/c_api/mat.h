#pragma once

#include "TruvixxAssimp/c_api/base.h"
#include "TruvixxAssimp/c_api/truvixx_assimp.export.h"

#ifdef __cplusplus
extern "C" {
#endif

/// 复制材质；外部纹理路径为完整 UTF-8 绝对路径，空字符串表示没有该纹理。
/// 路径含结尾 NUL 必须能装入对应数组；无法完整表示时返回 Fail，且不修改 out。
/// 显示名称可截断，不能据其结果重新定位文件。
ResType TRUVIXX_ASSIMP_API truvixx_material_get(TruvixxSceneHandle scene, uint32_t mat_index, TruvixxMat* out);

#ifdef __cplusplus
}
#endif
