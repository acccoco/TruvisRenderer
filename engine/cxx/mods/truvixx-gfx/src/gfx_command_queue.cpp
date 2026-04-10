#include "TruvixxGfx/gfx_command_queue.hpp"
#include "TruvixxGfx/gfx_device.hpp"

#include <stdexcept>

namespace truvixx
{

GfxCommandQueue::GfxCommandQueue(VkQueue queue, GfxQueueFamily queue_family, GfxDevice* device)
    : queue_(queue)
    , queue_family_(std::move(queue_family))
    , device_(device)
{
    // 加载扩展函数
    if (device != nullptr)
    {
        VkDevice vkDevice = device->handle();

        pfn_vkQueueSubmit2 = reinterpret_cast<PFN_vkQueueSubmit2>(
            vkGetDeviceProcAddr(vkDevice, "vkQueueSubmit2")
        );

        pfn_vkQueueBeginDebugUtilsLabel = reinterpret_cast<PFN_vkQueueBeginDebugUtilsLabelEXT>(
            vkGetDeviceProcAddr(vkDevice, "vkQueueBeginDebugUtilsLabelEXT")
        );

        pfn_vkQueueEndDebugUtilsLabel = reinterpret_cast<PFN_vkQueueEndDebugUtilsLabelEXT>(
            vkGetDeviceProcAddr(vkDevice, "vkQueueEndDebugUtilsLabelEXT")
        );

        pfn_vkQueueInsertDebugUtilsLabel = reinterpret_cast<PFN_vkQueueInsertDebugUtilsLabelEXT>(
            vkGetDeviceProcAddr(vkDevice, "vkQueueInsertDebugUtilsLabelEXT")
        );
    }
}

void GfxCommandQueue::waitIdle() const
{
    vkQueueWaitIdle(queue_);
}

void GfxCommandQueue::submit(const std::vector<VkSubmitInfo2>& submit_infos, VkFence fence) const
{
    if (pfn_vkQueueSubmit2 == nullptr)
    {
        throw std::runtime_error("vkQueueSubmit2 not available");
    }

    VkResult result = pfn_vkQueueSubmit2(
        queue_,
        static_cast<uint32_t>(submit_infos.size()),
        submit_infos.data(),
        fence
    );

    if (result != VK_SUCCESS)
    {
        throw std::runtime_error("Failed to submit command buffer");
    }
}

void GfxCommandQueue::begin_label(const std::string& label_name, float r, float g, float b, float a) const
{
    if (pfn_vkQueueBeginDebugUtilsLabel == nullptr)
    {
        return;
    }

    VkDebugUtilsLabelEXT label{};
    label.sType = VK_STRUCTURE_TYPE_DEBUG_UTILS_LABEL_EXT;
    label.pLabelName = label_name.c_str();
    label.color[0] = r;
    label.color[1] = g;
    label.color[2] = b;
    label.color[3] = a;

    pfn_vkQueueBeginDebugUtilsLabel(queue_, &label);
}

void GfxCommandQueue::end_label() const
{
    if (pfn_vkQueueEndDebugUtilsLabel == nullptr)
    {
        return;
    }

    pfn_vkQueueEndDebugUtilsLabel(queue_);
}

void GfxCommandQueue::insert_label(const std::string& label_name, float r, float g, float b, float a) const
{
    if (pfn_vkQueueInsertDebugUtilsLabel == nullptr)
    {
        return;
    }

    VkDebugUtilsLabelEXT label{};
    label.sType = VK_STRUCTURE_TYPE_DEBUG_UTILS_LABEL_EXT;
    label.pLabelName = label_name.c_str();
    label.color[0] = r;
    label.color[1] = g;
    label.color[2] = b;
    label.color[3] = a;

    pfn_vkQueueInsertDebugUtilsLabel(queue_, &label);
}

} // namespace truvixx
