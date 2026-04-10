#pragma once

#include <string>
#include <vector>

#include "vulkan/vulkan.hpp"

namespace truvixx
{

/// Vulkan Instance 封装
///
/// 管理 Vulkan 实例的创建和销毁，以及所需的 layers 和 extensions
struct GfxInstance
{
public:
    GfxInstance(
        const std::string& app_name,
        const std::string& engine_name,
        const std::vector<const char*>& extra_instance_exts = {}
    );

    ~GfxInstance();

    // 禁止拷贝
    GfxInstance(const GfxInstance&) = delete;
    GfxInstance& operator=(const GfxInstance&) = delete;

    // 允许移动
    GfxInstance(GfxInstance&& other) noexcept;
    GfxInstance& operator=(GfxInstance&& other) noexcept;

    [[nodiscard]] VkInstance handle() const { return instance_; }

private:
    /// 获取需要启用的 instance extensions
    std::vector<const char*> get_extensions(const std::vector<const char*>& extra_exts) const;

    /// 获取需要启用的 instance layers
    std::vector<const char*> get_layers() const;

    /// 必须要开启的 instance extensions
    static std::vector<const char*> basic_instance_exts();

    /// 必须要开启的 instance layers
    static std::vector<const char*> basic_instance_layers();

private:
    VkInstance instance_ = VK_NULL_HANDLE;
};

} // namespace truvixx
