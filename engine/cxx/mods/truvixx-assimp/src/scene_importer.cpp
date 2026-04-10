#include "TruvixxAssimp/scene_importer.hpp"

#include <assimp/Importer.hpp>
#include <assimp/postprocess.h>
#include <assimp/scene.h>
#include <assimp/matrix4x4.h>
#include <deque>
#include <format>
#include <iostream>

namespace truvixx
{

SceneImporter::SceneImporter()
    : importer_(std::make_unique<Assimp::Importer>())
{
}

SceneImporter::~SceneImporter() = default;

bool SceneImporter::load(const std::filesystem::path& path)
{
    // 清理之前的状态
    clear();

    // 验证文件存在
    if (!std::filesystem::exists(path) || !std::filesystem::is_regular_file(path))
    {
        std::cerr << std::format("File not found: {}", path.string()) << "\n";
        return false;
    }

    dir_ = path.parent_path();

    // Assimp 后处理标志
    // 坐标系：右手系，X-Right，Y-Up (Assimp 默认)
    // 三角形环绕：CCW (Assimp 默认)
    // UV 原点：左上角 (通过 FlipUVs)
    // 矩阵存储：row-major (Assimp 默认，转换时处理)
    constexpr unsigned int flags = aiProcess_CalcTangentSpace | // 生成切线空间
        aiProcess_JoinIdenticalVertices |                       // 去重顶点，生成索引
        aiProcess_Triangulate |                                 // 三角化
        aiProcess_GenNormals |                                  // 生成法线（如果没有）
        aiProcess_SortByPType |                                 // 按图元类型排序
        aiProcess_FlipUVs;                                      // UV 翻转为左上角原点

    // 加载场景
    ai_scene_ = importer_->ReadFile(path.string(), flags);

    if (!ai_scene_ || (ai_scene_->mFlags & AI_SCENE_FLAGS_INCOMPLETE) || !ai_scene_->mRootNode)
    {
        std::cerr << std::format("Assimp error: {}", importer_->GetErrorString()) << "\n";
        return false;
    }

    // 处理材质
    scene_data_.materials.reserve(ai_scene_->mNumMaterials);
    for (unsigned int i = 0; i < ai_scene_->mNumMaterials; ++i)
    {
        scene_data_.materials.emplace_back();
        process_material(ai_scene_->mMaterials[i], scene_data_.materials.back());
    }

    // 处理 Mesh
    scene_data_.mesh_infos.reserve(ai_scene_->mNumMeshes);
    for (unsigned int i = 0; i < ai_scene_->mNumMeshes; ++i)
    {
        scene_data_.mesh_infos.emplace_back();
        process_mesh_info(ai_scene_->mMeshes[i], scene_data_.mesh_infos.back());
    }

    // 处理节点树
    process_nodes(ai_scene_->mRootNode);

    is_loaded_ = true;
    return true;
}

const SceneData& SceneImporter::get_scene() const noexcept
{
    return scene_data_;
}

bool SceneImporter::is_loaded() const noexcept
{
    return is_loaded_;
}

TruvixxFloat3* SceneImporter::get_position(const uint32_t mesh_idx) const
{
    const auto ai_mesh = ai_scene_->mMeshes[mesh_idx];
    static_assert(sizeof(aiVector3D) == sizeof(TruvixxFloat3), "Size mismatch between aiVector3D and TruvixxFloat3");
    return reinterpret_cast<TruvixxFloat3*>(ai_mesh->mVertices);
}

TruvixxFloat3* SceneImporter::get_normal(const uint32_t mesh_idx) const
{
    const auto ai_mesh = ai_scene_->mMeshes[mesh_idx];
    static_assert(sizeof(aiVector3D) == sizeof(TruvixxFloat3), "Size mismatch between aiVector3D and TruvixxFloat3");
    return reinterpret_cast<TruvixxFloat3*>(ai_mesh->mNormals);
}

TruvixxFloat3* SceneImporter::get_tangent(const uint32_t mesh_idx) const
{
    const auto ai_mesh = ai_scene_->mMeshes[mesh_idx];
    static_assert(sizeof(aiVector3D) == sizeof(TruvixxFloat3), "Size mismatch between aiVector3D and TruvixxFloat3");
    return reinterpret_cast<TruvixxFloat3*>(ai_mesh->mTangents);
}

void SceneImporter::clear()
{
    scene_data_ = {};
    ai_scene_ = nullptr;
    is_loaded_ = false;

    // 重置 Importer（释放之前加载的场景）
    importer_ = std::make_unique<Assimp::Importer>();
}

void SceneImporter::process_nodes(const aiNode* root_node)
{
    if (!root_node)
        return;

    // BFS 遍历节点树
    std::deque<std::pair<const aiNode*, aiMatrix4x4>> queue;
    queue.emplace_back(root_node, aiMatrix4x4()); // 根节点，单位矩阵

    while (!queue.empty())
    {
        auto [node, parent_transform] = queue.front();
        queue.pop_front();

        // 处理当前节点
        process_node(node, parent_transform);

        // 计算当前累积变换
        aiMatrix4x4 current_transform = parent_transform * node->mTransformation;

        // 将子节点加入队列
        for (unsigned int i = 0; i < node->mNumChildren; ++i)
        {
            queue.emplace_back(node->mChildren[i], current_transform);
        }
    }
}

void SceneImporter::process_node(const aiNode* node, const aiMatrix4x4& parent_transform)
{
    if (!node)
        return;

    InstanceData instance;

    // 名称
    instance.name = node->mName.C_Str();

    // 世界变换矩阵 (Assimp row-major -> 我们 column-major)
    aiMatrix4x4 world = parent_transform * node->mTransformation;

    // 转换为列主序
    // Assimp: a1-a4 是第1行
    // 我们: m[0-3] 是第1列
    instance.world_transform.m[0] = world.a1;
    instance.world_transform.m[1] = world.b1;
    instance.world_transform.m[2] = world.c1;
    instance.world_transform.m[3] = world.d1;

    instance.world_transform.m[4] = world.a2;
    instance.world_transform.m[5] = world.b2;
    instance.world_transform.m[6] = world.c2;
    instance.world_transform.m[7] = world.d2;

    instance.world_transform.m[8] = world.a3;
    instance.world_transform.m[9] = world.b3;
    instance.world_transform.m[10] = world.c3;
    instance.world_transform.m[11] = world.d3;

    instance.world_transform.m[12] = world.a4;
    instance.world_transform.m[13] = world.b4;
    instance.world_transform.m[14] = world.c4;
    instance.world_transform.m[15] = world.d4;

    // Mesh 和材质引用
    instance.mesh_indices.reserve(node->mNumMeshes);
    instance.material_indices.reserve(node->mNumMeshes);

    for (unsigned int i = 0; i < node->mNumMeshes; ++i)
    {
        unsigned int mesh_idx = node->mMeshes[i];
        instance.mesh_indices.push_back(mesh_idx);
        instance.material_indices.push_back(ai_scene_->mMeshes[mesh_idx]->mMaterialIndex);
    }

    scene_data_.instances.push_back(std::move(instance));
}

void SceneImporter::process_mesh_info(const aiMesh* mesh, MeshInfo& out_mesh)
{
    if (!mesh)
        return;

    const unsigned int vertex_count = mesh->mNumVertices;
    const unsigned int face_count = mesh->mNumFaces;

    out_mesh.vertex_cnt = vertex_count;
    out_mesh.has_normal = mesh->HasNormals();
    out_mesh.has_tangent = mesh->HasTangentsAndBitangents();

    // UV (只取第一套)
    out_mesh.uvs.resize(static_cast<size_t>(vertex_count), { .x = 0.f, .y = 0.f });
    if (mesh->HasTextureCoords(0))
    {
        for (unsigned int i = 0; i < vertex_count; ++i)
        {
            out_mesh.uvs[i].x = mesh->mTextureCoords[0][i].x;
            out_mesh.uvs[i].y = mesh->mTextureCoords[0][i].y;
        }
    }

    // indices
    out_mesh.indices.reserve(static_cast<size_t>(face_count) * 3);
    for (unsigned int i = 0; i < face_count; ++i)
    {
        const aiFace& face = mesh->mFaces[i];
        if (face.mNumIndices != 3)
        {
            // 非三角形面，跳过
            continue;
        }
        out_mesh.indices.push_back(face.mIndices[0]);
        out_mesh.indices.push_back(face.mIndices[1]);
        out_mesh.indices.push_back(face.mIndices[2]);
    }
}

void SceneImporter::process_material(const aiMaterial* material, MaterialData& out_material) const
{
    if (!material)
        return;

    aiString out_str;
    aiColor4D out_color;
    ai_real out_real;
    // 纹理路径辅助函数
    auto get_texture_path = [&](const aiTextureType type) -> std::string {
        if (material->GetTextureCount(type) == 0)
            return {};

        aiString tex_path;
        if (material->GetTexture(type, 0, &tex_path) == AI_SUCCESS)
        {
            // 转换为绝对路径
            const std::filesystem::path full_path = dir_ / tex_path.C_Str();
            return full_path.string();
        }
        return {};
    };

    // name
    if (material->Get(AI_MATKEY_NAME, out_str) == AI_SUCCESS)
    {
        out_material.name = out_str.C_Str();
    }

    // base color
    if (material->Get(AI_MATKEY_COLOR_DIFFUSE, out_color) == AI_SUCCESS)
    {
        out_material.base_color.r = out_color.r;
        out_material.base_color.g = out_color.g;
        out_material.base_color.b = out_color.b;
        out_material.base_color.a = out_color.a;
    }

    // roughness
    if (material->Get(AI_MATKEY_ROUGHNESS_FACTOR, out_real) == AI_SUCCESS)
    {
        out_material.roughness = out_real;
    }

    // metallic
    if (material->Get(AI_MATKEY_REFLECTIVITY, out_real) == AI_SUCCESS)
    {
        out_material.metallic = out_real;
    }

    // emissive color
    if (material->Get(AI_MATKEY_COLOR_EMISSIVE, out_color) == AI_SUCCESS)
    {
        out_material.emissive.r = out_color.r;
        out_material.emissive.g = out_color.g;
        out_material.emissive.b = out_color.b;
        out_material.emissive.a = out_color.a;
    }

    // opacity
    if (material->Get(AI_MATKEY_OPACITY, out_real) == AI_SUCCESS)
    {
        out_material.opacity = out_real;
    }

    out_material.diffuse_map = get_texture_path(aiTextureType_DIFFUSE);
    out_material.normal_map = get_texture_path(aiTextureType_NORMALS);
}

} // namespace truvixx