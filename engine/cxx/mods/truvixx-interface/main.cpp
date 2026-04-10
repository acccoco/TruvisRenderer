#include <iostream>
#include <vector>
#include <format>

#include "TruvixxInterface/lib.h"

namespace
{
std::string format_float4(const TruvixxFloat4& vec)
{
    return std::format("({}, {}, {}, {})", vec.x, vec.y, vec.z, vec.w);
}

std::string format_float4x4(const TruvixxFloat4x4& mat)
{
    return std::format(
        "col 0: ({}, {}, {}, {})\n"
        "col 1: ({}, {}, {}, {})\n"
        "col 2: ({}, {}, {}, {})\n"
        "col 3: ({}, {}, {}, {})\n",
        mat.m[0], mat.m[1], mat.m[2], mat.m[3],
        mat.m[4], mat.m[5], mat.m[6], mat.m[7],
        mat.m[8], mat.m[9], mat.m[10], mat.m[11],
        mat.m[12], mat.m[13], mat.m[14], mat.m[15]
    );
}
} // namespace

int main(const int argc, char* argv[])
{
    if (argc < 2)
    {
        std::cerr << "Usage: " << argv[0] << " <path_to_scene_file>\n";
        return -1;
    }
    const auto scene = truvixx_scene_load(argv[1]);
    if (!scene)
    {
        std::cerr << "Failed to load scene." << "\n";
        return -1;
    }

    const auto mesh_cnt = truvixx_scene_mesh_count(scene);
    const auto mat_cnt = truvixx_scene_material_count(scene);
    const auto instance_cnt = truvixx_scene_instance_count(scene);

    std::cout << std::format("Instance count: {}\n", instance_cnt);
    std::cout << std::format("Mesh count: {}\n", mesh_cnt);
    std::cout << std::format("Material count: {}\n", mat_cnt);

    for (uint32_t instance_idx = 0; instance_idx < instance_cnt; ++instance_idx)
    {
        TruvixxInstance instance;
        if (!truvixx_instance_get(scene, instance_idx, &instance))
        {
            std::cerr << "Failed to get instance at index " << instance_idx << "\n";
            continue;
        }

        std::cout << "\n";
        std::cout << std::format("Instance (idx: {}, name: {})\n", instance_idx, instance.name);
        std::cout << std::format("World Transform:\n");
        std::cout << format_float4x4(instance.world_transform);
        std::cout << std::format("submesh count: {}\n", instance.mesh_count);

        std::vector<uint32_t> mesh_indices(instance.mesh_count);
        std::vector<uint32_t> mat_indices(instance.mesh_count);
        truvixx_instance_get_refs(scene, instance_idx, mesh_indices.data(), mat_indices.data());

        for (size_t submesh_idx = 0; submesh_idx < instance.mesh_count; ++submesh_idx)
        {
            std::cout << "submesh: " << submesh_idx << "\n";

            // 输出 mesh 的信息
            TruvixxMeshInfo mesh_info;
            if (!truvixx_mesh_get_info(scene, mesh_indices[submesh_idx], &mesh_info))
            {
                std::cerr << "Failed to get mesh at index " << mesh_indices[submesh_idx] << "\n";
                continue;
            }
            std::cout << std::format("Mesh: (global id: {})", mesh_indices[submesh_idx]) << "\n";
            std::cout << " vertex count: " << mesh_info.vertex_count << "\n";
            std::cout << " indices count: " << mesh_info.index_count << "\n";
            std::cout << " has normal: " << (mesh_info.has_normals ? "yes" : "no") << "\n";
            std::cout << " has tangent: " << (mesh_info.has_tangents ? "yes" : "no") << "\n";
            std::cout << " has uv: " << (mesh_info.has_uvs ? "yes" : "no") << "\n";

            // 输出 material 的信息
            TruvixxMat mat_info;
            if (!truvixx_material_get(scene, mat_indices[submesh_idx], &mat_info))
            {
                std::cerr << "Failed to get material at index " << mat_indices[submesh_idx] << "\n";
                continue;
            }
            auto mat_name = std::string(mat_info.name);
            std::cout << std::format("Material: (global idx: {}, name: {})", mat_indices[submesh_idx], mat_info.name) << "\n";
            std::cout << " base color: " << format_float4(mat_info.base_color) << "\n";
            std::cout << " roughness: " << mat_info.roughness << "\n";
            std::cout << " metallic: " << mat_info.metallic << "\n";
            std::cout << " Emissive color: " << format_float4(mat_info.emissive) << "\n";
            std::cout << " transmission factor: " << mat_info.opacity << "\n";

            std::cout << " base color texture: " << mat_info.diffuse_map << "\n";
            std::cout << " normal texture: " << mat_info.normal_map << "\n";
        }
    }

    truvixx_scene_free(scene);

    return 0;
}