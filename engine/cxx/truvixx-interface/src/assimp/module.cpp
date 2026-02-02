#include "TruvixxInterface/assimp/module.h"
#include "TruvixxAssimp/scene_importer.hpp"

#include <algorithm>

/// 场景句柄的实际类型
struct TruvixxScene
{
    truvixx::SceneImporter importer;
};

namespace
{

/// 安全复制字符串到固定大小缓冲区
void safe_strcpy(char* dest, const size_t dest_size, const std::string& src)
{
    if (dest_size == 0)
        return;

    size_t copy_len = std::min(src.size(), dest_size - 1);
    std::memcpy(dest, src.data(), copy_len);
    dest[copy_len] = '\0';
}

/// 获取场景数据 (带空检查)
const truvixx::SceneData* get_scene_data(TruvixxSceneHandle scene)
{
    if (!scene || !scene->importer.is_loaded())
        return nullptr;
    return &scene->importer.get_scene();
}

} // namespace

TruvixxSceneHandle truvixx_scene_load(const char* path)
{
    if (!path)
        return nullptr;

    auto* scene = new TruvixxScene;
    if (!scene->importer.load(path))
    {
        // 保留错误信息，不立即删除
        return scene;
    }
    return scene;
}

void truvixx_scene_free(const TruvixxSceneHandle scene)
{
    delete scene;
}

uint32_t truvixx_scene_mesh_count(const TruvixxSceneHandle scene)
{
    const auto* data = get_scene_data(scene);
    return data ? data->mesh_count() : 0;
}

uint32_t truvixx_scene_material_count(const TruvixxSceneHandle scene)
{
    const auto* data = get_scene_data(scene);
    return data ? data->material_count() : 0;
}

uint32_t truvixx_scene_instance_count(const TruvixxSceneHandle scene)
{
    const auto* data = get_scene_data(scene);
    return data ? data->instance_count() : 0;
}

ResType truvixx_material_get(const TruvixxSceneHandle scene, const uint32_t mat_index, TruvixxMat* out)
{
    if (!out)
        return ResTypeFail;

    const auto* data = get_scene_data(scene);
    if (!data || mat_index >= data->material_count())
        return ResTypeFail;

    const auto& mat = data->materials[mat_index];

    safe_strcpy(out->name, sizeof(out->name), mat.name);

    out->base_color = mat.base_color;
    out->roughness = mat.roughness;
    out->metallic = mat.metallic;
    out->emissive = mat.emissive;
    out->opacity = mat.opacity;

    safe_strcpy(out->diffuse_map, sizeof(out->diffuse_map), mat.diffuse_map);
    safe_strcpy(out->normal_map, sizeof(out->normal_map), mat.normal_map);

    return ResTypeSuccess;
}

ResType truvixx_instance_get(const TruvixxSceneHandle scene, const uint32_t index, TruvixxInstance* out)
{
    if (!out)
        return ResTypeFail;

    const auto* data = get_scene_data(scene);
    if (!data || index >= data->instance_count())
        return ResTypeFail;

    const auto& inst = data->instances[index];

    safe_strcpy(out->name, sizeof(out->name), inst.name);
    out->world_transform = inst.world_transform;
    out->mesh_count = inst.mesh_count();

    return ResTypeSuccess;
}

ResType truvixx_instance_get_refs(
    const TruvixxSceneHandle scene,
    const uint32_t instance_index,
    uint32_t* out_mesh_indices,
    uint32_t* out_material_indices
)
{
    const auto* data = get_scene_data(scene);
    if (!data || instance_index >= data->instance_count())
        return ResTypeFail;

    const auto& inst = data->instances[instance_index];

    if (out_mesh_indices)
    {
        std::memcpy(out_mesh_indices, inst.mesh_indices.data(), inst.mesh_indices.size() * sizeof(uint32_t));
    }

    if (out_material_indices)
    {
        std::memcpy(out_material_indices, inst.material_indices.data(), inst.material_indices.size() * sizeof(uint32_t));
    }

    return ResTypeSuccess;
}

ResType truvixx_mesh_get_info(const TruvixxSceneHandle scene, const uint32_t mesh_index, TruvixxMeshInfo* out)
{
    if (!out)
        return ResTypeFail;

    const auto* data = get_scene_data(scene);
    if (!data || mesh_index >= data->mesh_count())
        return ResTypeFail;

    const auto& mesh_info = data->mesh_infos[mesh_index];

    out->vertex_count = mesh_info.vertex_cnt;
    out->index_count = static_cast<uint32_t>(mesh_info.indices.size());
    out->has_normals = mesh_info.has_normal;
    out->has_tangents = mesh_info.has_tangent;
    out->has_uvs = !mesh_info.uvs.empty();

    return ResTypeSuccess;
}

ResType truvixx_mesh_fill_positions(const TruvixxSceneHandle scene, const uint32_t mesh_index, float* out)
{
    if (!out)
        return ResTypeFail;

    const auto* scene_data = get_scene_data(scene);
    if (!scene_data || mesh_index >= scene_data->mesh_count())
        return ResTypeFail;

    const auto position_ptr = scene->importer.get_position(mesh_index);
    if (!position_ptr)
        return ResTypeFail;

    const auto& mesh_info = scene_data->mesh_infos[mesh_index];
    std::memcpy(out, position_ptr, mesh_info.vertex_cnt * sizeof(TruvixxFloat3));

    return ResTypeSuccess;
}

ResType truvixx_mesh_fill_normals(const TruvixxSceneHandle scene, const uint32_t mesh_index, float* out)
{
    if (!out)
        return ResTypeFail;

    const auto* scene_data = get_scene_data(scene);
    if (!scene_data || mesh_index >= scene_data->mesh_count())
        return ResTypeFail;

    const auto& mesh_info = scene_data->mesh_infos[mesh_index];
    const auto normal_ptr = scene->importer.get_normal(mesh_index);
    if (!mesh_info.has_normal || !normal_ptr)
        return ResTypeFail;

    std::memcpy(out, normal_ptr, mesh_info.vertex_cnt * sizeof(TruvixxFloat3));

    return ResTypeSuccess;
}

ResType truvixx_mesh_fill_tangents(const TruvixxSceneHandle scene, const uint32_t mesh_index, float* out)
{
    if (!out)
        return ResTypeFail;

    const auto* scene_data = get_scene_data(scene);
    if (!scene_data || mesh_index >= scene_data->mesh_count())
        return ResTypeFail;

    const auto& mesh_info = scene_data->mesh_infos[mesh_index];
    const auto tangent_ptr = scene->importer.get_normal(mesh_index);
    if (!mesh_info.has_tangent || !tangent_ptr)
        return ResTypeFail;

    std::memcpy(out, tangent_ptr, mesh_info.vertex_cnt * sizeof(TruvixxFloat3));

    return ResTypeSuccess;
}

ResType truvixx_mesh_fill_uvs(const TruvixxSceneHandle scene, const uint32_t mesh_index, float* out)
{
    if (!out)
        return ResTypeFail;

    const auto* scene_data = get_scene_data(scene);
    if (!scene_data || mesh_index >= scene_data->mesh_count())
        return ResTypeFail;

    const auto& mesh_info = scene_data->mesh_infos[mesh_index];
    if (mesh_info.uvs.empty())
        return ResTypeFail;

    std::memcpy(out, mesh_info.uvs.data(), mesh_info.uvs.size() * sizeof(TruvixxFloat2));

    return ResTypeSuccess;
}

ResType truvixx_mesh_fill_indices(const TruvixxSceneHandle scene, const uint32_t mesh_index, uint32_t* out)
{
    if (!out)
        return ResTypeFail;

    const auto* scene_data = get_scene_data(scene);
    if (!scene_data || mesh_index >= scene_data->mesh_count())
        return ResTypeFail;

    const auto& mesh_info = scene_data->mesh_infos[mesh_index];
    if (mesh_info.indices.empty())
        return ResTypeFail;

    std::memcpy(out, mesh_info.indices.data(), mesh_info.indices.size() * sizeof(uint32_t));

    return ResTypeSuccess;
}

const TruvixxFloat3* truvixx_mesh_get_positions(const TruvixxSceneHandle scene, const uint32_t mesh_index)
{
    const auto* data = get_scene_data(scene);
    if (!data || mesh_index >= data->mesh_count())
        return nullptr;

    const auto& mesh_info = data->mesh_infos[mesh_index];
    const auto position_ptr = scene->importer.get_position(mesh_index);

    return mesh_info.vertex_cnt == 0 ? nullptr : position_ptr;
}

const TruvixxFloat3* truvixx_mesh_get_normals(const TruvixxSceneHandle scene, const uint32_t mesh_index)
{
    const auto* data = get_scene_data(scene);
    if (!data || mesh_index >= data->mesh_count())
        return nullptr;

    const auto& mesh_info = data->mesh_infos[mesh_index];
    const auto normal_ptr = scene->importer.get_normal(mesh_index);

    return mesh_info.has_normal ? normal_ptr : nullptr;
}

const TruvixxFloat3* truvixx_mesh_get_tangents(const TruvixxSceneHandle scene, const uint32_t mesh_index)
{
    const auto* data = get_scene_data(scene);
    if (!data || mesh_index >= data->mesh_count())
        return nullptr;

    const auto& mesh_info = data->mesh_infos[mesh_index];
    const auto tangent_ptr = scene->importer.get_tangent(mesh_index);

    return mesh_info.has_tangent ? tangent_ptr : nullptr;
}

const TruvixxFloat2* truvixx_mesh_get_uvs(const TruvixxSceneHandle scene, const uint32_t mesh_index)
{
    const auto* data = get_scene_data(scene);
    if (!data || mesh_index >= data->mesh_count())
        return nullptr;

    const auto& mesh_info = data->mesh_infos[mesh_index];
    return mesh_info.uvs.empty() ? nullptr : mesh_info.uvs.data();
}

const uint32_t* truvixx_mesh_get_indices(const TruvixxSceneHandle scene, const uint32_t mesh_index)
{
    const auto* data = get_scene_data(scene);
    if (!data || mesh_index >= data->mesh_count())
        return nullptr;

    const auto& mesh_info = data->mesh_infos[mesh_index];
    return mesh_info.indices.empty() ? nullptr : mesh_info.indices.data();
}
