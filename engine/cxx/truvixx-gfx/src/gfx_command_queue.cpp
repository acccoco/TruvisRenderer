#include "TruvixxGfx/gfx_command_queue.hpp"
#include "TruvixxGfx/gfx_device.hpp"

#include <stdexcept>

namespace truvixx {

GfxCommandQueue::GfxCommandQueue(VkQueue queue, GfxQueueFamily queueFamily, GfxDevice* device)
    : m_queue(queue)
    , m_queueFamily(std::move(queueFamily))
    , m_device(device)
{
    // 加载扩展函数
    if (device != nullptr) {
        VkDevice vkDevice = device->handle();

        m_vkQueueSubmit2 = reinterpret_cast<PFN_vkQueueSubmit2>(
            vkGetDeviceProcAddr(vkDevice, "vkQueueSubmit2"));

        m_vkQueueBeginDebugUtilsLabel = reinterpret_cast<PFN_vkQueueBeginDebugUtilsLabelEXT>(
            vkGetDeviceProcAddr(vkDevice, "vkQueueBeginDebugUtilsLabelEXT"));

        m_vkQueueEndDebugUtilsLabel = reinterpret_cast<PFN_vkQueueEndDebugUtilsLabelEXT>(
            vkGetDeviceProcAddr(vkDevice, "vkQueueEndDebugUtilsLabelEXT"));

        m_vkQueueInsertDebugUtilsLabel = reinterpret_cast<PFN_vkQueueInsertDebugUtilsLabelEXT>(
            vkGetDeviceProcAddr(vkDevice, "vkQueueInsertDebugUtilsLabelEXT"));
    }
}

void GfxCommandQueue::waitIdle() const
{
    vkQueueWaitIdle(m_queue);
}

void GfxCommandQueue::submit(const std::vector<VkSubmitInfo2>& submitInfos, VkFence fence) const
{
    if (m_vkQueueSubmit2 == nullptr) {
        throw std::runtime_error("vkQueueSubmit2 not available");
    }

    VkResult result = m_vkQueueSubmit2(
        m_queue,
        static_cast<uint32_t>(submitInfos.size()),
        submitInfos.data(),
        fence);

    if (result != VK_SUCCESS) {
        throw std::runtime_error("Failed to submit command buffer");
    }
}

void GfxCommandQueue::beginLabel(const std::string& labelName, float r, float g, float b, float a) const
{
    if (m_vkQueueBeginDebugUtilsLabel == nullptr) {
        return;
    }

    VkDebugUtilsLabelEXT label{};
    label.sType = VK_STRUCTURE_TYPE_DEBUG_UTILS_LABEL_EXT;
    label.pLabelName = labelName.c_str();
    label.color[0] = r;
    label.color[1] = g;
    label.color[2] = b;
    label.color[3] = a;

    m_vkQueueBeginDebugUtilsLabel(m_queue, &label);
}

void GfxCommandQueue::endLabel() const
{
    if (m_vkQueueEndDebugUtilsLabel == nullptr) {
        return;
    }

    m_vkQueueEndDebugUtilsLabel(m_queue);
}

void GfxCommandQueue::insertLabel(const std::string& labelName, float r, float g, float b, float a) const
{
    if (m_vkQueueInsertDebugUtilsLabel == nullptr) {
        return;
    }

    VkDebugUtilsLabelEXT label{};
    label.sType = VK_STRUCTURE_TYPE_DEBUG_UTILS_LABEL_EXT;
    label.pLabelName = labelName.c_str();
    label.color[0] = r;
    label.color[1] = g;
    label.color[2] = b;
    label.color[3] = a;

    m_vkQueueInsertDebugUtilsLabel(m_queue, &label);
}

} // namespace truvixx
