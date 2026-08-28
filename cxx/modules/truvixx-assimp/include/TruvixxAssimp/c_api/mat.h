#pragma once

#include "TruvixxAssimp/c_api/base.h"
#include "TruvixxAssimp/c_api/truvixx_assimp.export.h"

#ifdef __cplusplus
extern "C" {
#endif

ResType TRUVIXX_ASSIMP_API truvixx_material_get(TruvixxSceneHandle scene, uint32_t mat_index, TruvixxMat* out);

#ifdef __cplusplus
}
#endif
