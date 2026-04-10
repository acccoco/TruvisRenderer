#pragma once

#include "TruvixxInterface/assimp/base.h"
#include "TruvixxInterface/truvixx_interface.export.h"

#ifdef __cplusplus
extern "C" {
#endif

ResType TRUVIXX_INTERFACE_API truvixx_material_get(TruvixxSceneHandle scene, uint32_t mat_index, TruvixxMat* out);

#ifdef __cplusplus
}
#endif
