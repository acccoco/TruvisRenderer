#pragma once

#include "TruvixxAssimp/scene_data.hpp"

#include <filesystem>
#include <memory>
#include <assimp/Importer.hpp>
#include <assimp/scene.h>

namespace truvixx
{

struct SceneImporter
{
public:
    SceneImporter();
    ~SceneImporter();

    // 禁止拷贝和移动 (持有 Assimp::Importer)
    SceneImporter(const SceneImporter&) = delete;
    SceneImporter& operator=(const SceneImporter&) = delete;
    SceneImporter(SceneImporter&&) = delete;
    SceneImporter& operator=(SceneImporter&&) = delete;

public:
    /// 加载场景文件
    /// @param path 场景文件路径
    /// @return 成功返回 true
    [[nodiscard]] bool load(const std::filesystem::path& path);

    /// 获取加载后的场景数据 (只读引用)
    [[nodiscard]] const SceneData& get_scene() const noexcept;

    /// 是否已成功加载场景
    [[nodiscard]] bool is_loaded() const noexcept;

    TruvixxFloat3* get_position(uint32_t mesh_idx) const;
    TruvixxFloat3* get_normal(uint32_t mesh_idx) const;
    TruvixxFloat3* get_tangent(uint32_t mesh_idx) const;

    /// 清空已加载的数据
    void clear();

private:
    /// 处理场景树中的所有节点
    void process_nodes(const aiNode* root_node);

    /// 处理单个节点
    void process_node(const aiNode* node, const aiMatrix4x4& parent_transform);

    /// 处理 Mesh
    static void process_mesh_info(const aiMesh* mesh, MeshInfo& out_mesh);

    /// 处理材质
    void process_material(const aiMaterial* material, MaterialData& out_material) const;

private:
    std::unique_ptr<Assimp::Importer> importer_; ///< Assimp 导入器，持有 ai_scene 生命周期
    const aiScene* ai_scene_ = nullptr;          ///< Assimp 场景 (由 importer_ 管理)

    SceneData scene_data_;      ///< 转换后的场景数据
    std::filesystem::path dir_; ///< 场景文件所在目录
    bool is_loaded_ = false;    ///< 加载状态
};

} // namespace truvixx