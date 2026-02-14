#pragma once

#include "TruvixxGfx/gfx_physical_device.hpp"

namespace truvixx
{

struct GfxDevice;

/// Command Queue 封装
///
/// 管理 Vulkan 队列的提交和同步操作
struct GfxCommandQueue
{
public:
    GfxCommandQueue(VkQueue queue, GfxQueueFamily queue_family, GfxDevice* device);

    ~GfxCommandQueue() = default;

    // 禁止拷贝
    GfxCommandQueue(const GfxCommandQueue&) = delete;
    GfxCommandQueue& operator=(const GfxCommandQueue&) = delete;

    // 允许移动
    GfxCommandQueue(GfxCommandQueue&&) noexcept = default;
    GfxCommandQueue& operator=(GfxCommandQueue&&) noexcept = default;

    [[nodiscard]] VkQueue handle() const { return queue_; }
    [[nodiscard]] const GfxQueueFamily& queue_family() const { return queue_family_; }

    /// 等待队列空闲
    void waitIdle() const;

    /// 提交命令缓冲区
    void submit(const std::vector<VkSubmitInfo2>& submit_infos, VkFence fence = VK_NULL_HANDLE) const;

    // Debug label 相关
    void begin_label(const std::string& label_name, float r, float g, float b, float a) const;
    void end_label() const;
    void insert_label(const std::string& label_name, float r, float g, float b, float a) const;

private:
    VkQueue queue_ = VK_NULL_HANDLE;
    GfxQueueFamily queue_family_;
    GfxDevice* device_ = nullptr;

    // 扩展函数指针
    PFN_vkQueueSubmit2 pfn_vkQueueSubmit2 = nullptr;
    PFN_vkQueueBeginDebugUtilsLabelEXT pfn_vkQueueBeginDebugUtilsLabel = nullptr;
    PFN_vkQueueEndDebugUtilsLabelEXT pfn_vkQueueEndDebugUtilsLabel = nullptr;
    PFN_vkQueueInsertDebugUtilsLabelEXT pfn_vkQueueInsertDebugUtilsLabel = nullptr;
};

} // namespace truvixx
