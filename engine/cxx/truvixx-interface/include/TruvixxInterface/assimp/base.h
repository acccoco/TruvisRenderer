#pragma once

#include "TruvixxAssimp/base_type.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef unsigned int uint32_t;
typedef int int32_t;

typedef enum : uint32_t
{
    ResTypeFail = 0,
    ResTypeSuccess = 1,
} ResType;

/// 场景句柄 (不透明指针)
typedef struct TruvixxScene* TruvixxSceneHandle;

/// 材质信息
typedef struct
{
    char name[256];

    TruvixxFloat4 base_color;
    float roughness;
    TruvixxFloat4 emissive;
    float metallic;
    float opacity;

    char diffuse_map[256];
    char normal_map[256];
} TruvixxMat;

/// Instance 信息
typedef struct
{
    char name[256];
    TruvixxFloat4x4 world_transform; ///< 世界变换矩阵
    unsigned int mesh_count;
} TruvixxInstance;

/// Mesh 元信息 (用于预分配 buffer)
typedef struct
{
    uint32_t vertex_count;
    uint32_t index_count;

    uint32_t has_normals;
    uint32_t has_tangents;
    uint32_t has_uvs;
} TruvixxMeshInfo;

#ifdef __cplusplus
}
#endif
