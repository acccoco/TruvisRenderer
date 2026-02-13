#pragma once

#include "TruvixxGfx/gfx_physical_device.hpp"

#include <memory>

namespace truvixx {

class GfxDevice;

/// Command Queue 封装
///
/// 管理 Vulkan 队列的提交和同步操作
class GfxCommandQueue {
public:
    GfxCommandQueue(VkQueue queue, GfxQueueFamily queueFamily, GfxDevice* device);

    ~GfxCommandQueue() = default;

    // 禁止拷贝
    GfxCommandQueue(const GfxCommandQueue&) = delete;
    GfxCommandQueue& operator=(const GfxCommandQueue&) = delete;

    // 允许移动
    GfxCommandQueue(GfxCommandQueue&&) noexcept = default;
    GfxCommandQueue& operator=(GfxCommandQueue&&) noexcept = default;

    [[nodiscard]] VkQueue handle() const { return m_queue; }
    [[nodiscard]] const GfxQueueFamily& queueFamily() const { return m_queueFamily; }

    /// 等待队列空闲
    void waitIdle() const;

    /// 提交命令缓冲区
    void submit(const std::vector<VkSubmitInfo2>& submitInfos, VkFence fence = VK_NULL_HANDLE) const;

    // Debug label 相关
    void beginLabel(const std::string& labelName, float r, float g, float b, float a) const;
    void endLabel() const;
    void insertLabel(const std::string& labelName, float r, float g, float b, float a) const;

private:
    VkQueue m_queue = VK_NULL_HANDLE;
    GfxQueueFamily m_queueFamily;
    GfxDevice* m_device = nullptr;

    // 扩展函数指针
    PFN_vkQueueSubmit2 m_vkQueueSubmit2 = nullptr;
    PFN_vkQueueBeginDebugUtilsLabelEXT m_vkQueueBeginDebugUtilsLabel = nullptr;
    PFN_vkQueueEndDebugUtilsLabelEXT m_vkQueueEndDebugUtilsLabel = nullptr;
    PFN_vkQueueInsertDebugUtilsLabelEXT m_vkQueueInsertDebugUtilsLabel = nullptr;
};

} // namespace truvixx
