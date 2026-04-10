#pragma once

#include "TruvixxAssimp/base_type.h"

#include <cstdint>
#include <string>
#include <vector>

namespace truvixx
{

inline constexpr size_t MAX_NAME_LENGTH = 256;

/// PBR 材质数据
struct MaterialData
{
    std::string name;

    // PBR 参数
    TruvixxFloat4 base_color = { { 1.f, 1.f, 1.f, 1.f } };
    float roughness = 0.5f;
    float metallic = 0.0f;
    TruvixxFloat4 emissive = { { 0.0f, 0.0f, 0.0f, 1.0f } };
    float opacity = 1.0f; ///< 1 = opaque, 0 = transparent

    // 纹理路径 (绝对路径)
    std::string diffuse_map;
    std::string normal_map;
};

/// 场景实例 (节点)
struct InstanceData
{
    std::string name;

    /// 世界变换矩阵 (列主序, 4x4)
    /// 坐标系：右手系，X-Right，Y-Up
    TruvixxFloat4x4 world_transform = {
        { 1, 0, 0, 0,
          0, 1, 0, 0,
          0, 0, 1, 0,
          0, 0, 0, 1 }
    };

    /// 该实例引用的 mesh 索引列表
    std::vector<uint32_t> mesh_indices;

    /// 该实例引用的材质索引列表 (与 mesh_indices 一一对应)
    std::vector<uint32_t> material_indices;

    [[nodiscard]]
    uint32_t mesh_count() const noexcept
    {
        return static_cast<uint32_t>(mesh_indices.size());
    }
};

struct MeshInfo
{
    uint32_t vertex_cnt;

    std::vector<TruvixxFloat2> uvs;
    std::vector<uint32_t> indices;
    bool has_normal;
    bool has_tangent;
};

/// 场景容器，持有所有 mesh、材质和实例数据
struct SceneData
{
    std::vector<MeshInfo> mesh_infos;
    std::vector<MaterialData> materials;
    std::vector<InstanceData> instances;

    [[nodiscard]]
    uint32_t mesh_count() const noexcept
    {
        return static_cast<uint32_t>(mesh_infos.size());
    }

    [[nodiscard]]
    uint32_t material_count() const noexcept
    {
        return static_cast<uint32_t>(materials.size());
    }

    [[nodiscard]]
    uint32_t instance_count() const noexcept
    {
        return static_cast<uint32_t>(instances.size());
    }
};

} // namespace truvixx