#pragma once

#include <string>
#include <vector>

#include "vulkan/vulkan.hpp"

namespace truvixx {

/// Vulkan Instance 封装
///
/// 管理 Vulkan 实例的创建和销毁，以及所需的 layers 和 extensions
class GfxInstance {
public:
    GfxInstance(const std::string& appName,
                const std::string& engineName,
                const std::vector<const char*>& extraInstanceExts = {});

    ~GfxInstance();

    // 禁止拷贝
    GfxInstance(const GfxInstance&) = delete;
    GfxInstance& operator=(const GfxInstance&) = delete;

    // 允许移动
    GfxInstance(GfxInstance&& other) noexcept;
    GfxInstance& operator=(GfxInstance&& other) noexcept;

    [[nodiscard]] VkInstance handle() const { return m_instance; }

private:
    /// 获取需要启用的 instance extensions
    std::vector<const char*> getExtensions(const std::vector<const char*>& extraExts) const;

    /// 获取需要启用的 instance layers
    std::vector<const char*> getLayers() const;

    /// 必须要开启的 instance extensions
    static std::vector<const char*> basicInstanceExts();

    /// 必须要开启的 instance layers
    static std::vector<const char*> basicInstanceLayers();

private:
    VkInstance m_instance = VK_NULL_HANDLE;
};

} // namespace truvixx
