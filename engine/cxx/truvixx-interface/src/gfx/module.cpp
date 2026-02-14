#include "TruvixxInterface/gfx/module.h"
#include "TruvixxGfx/lib.hpp"

#include <algorithm>
#include <cstring>
#include <vector>

//=============================================================================
// 不透明句柄的实际类型定义
//=============================================================================

struct TruvixxGfxInstance
{
    truvixx::GfxInstance impl;

    TruvixxGfxInstance(
        const std::string& app_name,
        const std::string& engine_name,
        const std::vector<const char*>& extra_exts
    )
        : impl(app_name, engine_name, extra_exts)
    {
    }
};

struct TruvixxGfxPhysicalDevice
{
    truvixx::GfxPhysicalDevice impl;

    explicit TruvixxGfxPhysicalDevice(VkInstance instance)
        : impl(instance)
    {
    }
};

struct TruvixxGfxDevice
{
    truvixx::GfxDevice impl;

    TruvixxGfxDevice(
        VkInstance instance,
        VkPhysicalDevice pdevice,
        const std::vector<VkDeviceQueueCreateInfo>& queue_infos
    )
        : impl(instance, pdevice, queue_infos)
    {
    }
};

struct TruvixxGfxSwapchain
{
    truvixx::GfxSwapchain impl;

    TruvixxGfxSwapchain(
        VkPhysicalDevice pdevice,
        truvixx::GfxDevice* device,
        VkSurfaceKHR surface,
        VkPresentModeKHR present_mode,
        VkSurfaceFormatKHR surface_format,
        VkExtent2D extent,
        VkSwapchainKHR old_swapchain
    )
        : impl(pdevice, device, surface, present_mode, surface_format, extent, old_swapchain)
    {
    }
};

struct TruvixxGfxCommandQueue
{
    truvixx::GfxCommandQueue impl;

    TruvixxGfxCommandQueue(VkQueue queue, truvixx::GfxQueueFamily family, truvixx::GfxDevice* device)
        : impl(queue, family, device)
    {
    }
};

//=============================================================================
// 辅助函数
//=============================================================================

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

/// 将 C 结构体转换为 C++ 结构体
truvixx::GfxQueueFamily convert_queue_family(const TruvixxGfxQueueFamily& c_family)
{
    truvixx::GfxQueueFamily family;
    family.name = c_family.name;
    family.queue_family_index = c_family.queue_family_index;
    family.queue_flags = static_cast<VkQueueFlags>(c_family.queue_flags);
    family.queue_count = c_family.queue_count;
    return family;
}

/// 将 C++ 结构体转换为 C 结构体
void fill_queue_family(TruvixxGfxQueueFamily* out, const truvixx::GfxQueueFamily& family)
{
    safe_strcpy(out->name, sizeof(out->name), family.name);
    out->queue_family_index = family.queue_family_index;
    out->queue_flags = static_cast<uint32_t>(family.queue_flags);
    out->queue_count = family.queue_count;
}

} // namespace

//=============================================================================
// GfxInstance C API 实现
//=============================================================================

TruvixxGfxInstanceHandle truvixx_gfx_instance_create(
    const char* app_name,
    const char* engine_name,
    const char** extra_exts,
    const uint32_t ext_count
)
{
    try
    {
        std::vector<const char*> exts;
        if (extra_exts && ext_count > 0)
        {
            exts.assign(extra_exts, extra_exts + ext_count);
        }

        return new TruvixxGfxInstance(
            app_name ? app_name : "",
            engine_name ? engine_name : "",
            exts
        );
    } catch (...)
    {
        return nullptr;
    }
}

void truvixx_gfx_instance_destroy(const TruvixxGfxInstanceHandle instance)
{
    delete instance;
}

uint64_t truvixx_gfx_instance_handle(const TruvixxGfxInstanceHandle instance)
{
    if (!instance)
        return 0;
    return reinterpret_cast<uint64_t>(instance->impl.handle());
}

//=============================================================================
// GfxPhysicalDevice C API 实现
//=============================================================================

TruvixxGfxPhysicalDeviceHandle truvixx_gfx_physical_device_create(const uint64_t vk_instance)
{
    if (vk_instance == 0)
        return nullptr;

    try
    {
        return new TruvixxGfxPhysicalDevice(reinterpret_cast<VkInstance>(vk_instance));
    } catch (...)
    {
        return nullptr;
    }
}

void truvixx_gfx_physical_device_destroy(const TruvixxGfxPhysicalDeviceHandle pdevice)
{
    delete pdevice;
}

uint64_t truvixx_gfx_physical_device_handle(const TruvixxGfxPhysicalDeviceHandle pdevice)
{
    if (!pdevice)
        return 0;
    return reinterpret_cast<uint64_t>(pdevice->impl.handle());
}

TruvixxGfxResult truvixx_gfx_physical_device_gfx_queue_family(
    const TruvixxGfxPhysicalDeviceHandle pdevice,
    TruvixxGfxQueueFamily* out
)
{
    if (!pdevice || !out)
        return TruvixxGfxResultFail;

    fill_queue_family(out, pdevice->impl.gfx_queue_family());
    return TruvixxGfxResultSuccess;
}

uint32_t truvixx_gfx_physical_device_has_compute_queue_family(const TruvixxGfxPhysicalDeviceHandle pdevice)
{
    if (!pdevice)
        return 0;
    return pdevice->impl.compute_queue_family().has_value() ? 1 : 0;
}

TruvixxGfxResult truvixx_gfx_physical_device_compute_queue_family(
    const TruvixxGfxPhysicalDeviceHandle pdevice,
    TruvixxGfxQueueFamily* out
)
{
    if (!pdevice || !out)
        return TruvixxGfxResultFail;

    const auto& opt = pdevice->impl.compute_queue_family();
    if (!opt.has_value())
        return TruvixxGfxResultFail;

    fill_queue_family(out, *opt);
    return TruvixxGfxResultSuccess;
}

uint32_t truvixx_gfx_physical_device_has_transfer_queue_family(const TruvixxGfxPhysicalDeviceHandle pdevice)
{
    if (!pdevice)
        return 0;
    return pdevice->impl.transfer_queue_family().has_value() ? 1 : 0;
}

TruvixxGfxResult truvixx_gfx_physical_device_transfer_queue_family(
    const TruvixxGfxPhysicalDeviceHandle pdevice,
    TruvixxGfxQueueFamily* out
)
{
    if (!pdevice || !out)
        return TruvixxGfxResultFail;

    const auto& opt = pdevice->impl.transfer_queue_family();
    if (!opt.has_value())
        return TruvixxGfxResultFail;

    fill_queue_family(out, *opt);
    return TruvixxGfxResultSuccess;
}

uint32_t truvixx_gfx_physical_device_is_discrete_gpu(const TruvixxGfxPhysicalDeviceHandle pdevice)
{
    if (!pdevice)
        return 0;
    return pdevice->impl.is_discrete_gpu() ? 1 : 0;
}

void truvixx_gfx_physical_device_name(
    const TruvixxGfxPhysicalDeviceHandle pdevice,
    char* out_name,
    const uint32_t buffer_size
)
{
    if (!pdevice || !out_name || buffer_size == 0)
        return;

    safe_strcpy(out_name, buffer_size, pdevice->impl.properties().deviceName);
}

//=============================================================================
// GfxDevice C API 实现
//=============================================================================

TruvixxGfxDeviceHandle truvixx_gfx_device_create(
    const uint64_t vk_instance,
    const uint64_t vk_physical_device,
    const TruvixxGfxDeviceQueueCreateInfo* queue_infos,
    const uint32_t queue_info_count
)
{
    if (vk_instance == 0 || vk_physical_device == 0)
        return nullptr;

    try
    {
        std::vector<VkDeviceQueueCreateInfo> vk_queue_infos;
        std::vector<std::vector<float>> priorities_storage; // 保持优先级数组的生命周期

        if (queue_infos && queue_info_count > 0)
        {
            vk_queue_infos.reserve(queue_info_count);
            priorities_storage.reserve(queue_info_count);

            for (uint32_t i = 0; i < queue_info_count; ++i)
            {
                const auto& info = queue_infos[i];

                // 复制优先级数组
                std::vector<float> priorities(info.queue_count, 1.0f);
                if (info.priorities)
                {
                    for (uint32_t j = 0; j < info.queue_count; ++j)
                    {
                        priorities[j] = info.priorities[j];
                    }
                }
                priorities_storage.push_back(std::move(priorities));

                VkDeviceQueueCreateInfo vk_info{};
                vk_info.sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO;
                vk_info.queueFamilyIndex = info.queue_family_index;
                vk_info.queueCount = info.queue_count;
                vk_info.pQueuePriorities = priorities_storage.back().data();

                vk_queue_infos.push_back(vk_info);
            }
        }

        return new TruvixxGfxDevice(
            reinterpret_cast<VkInstance>(vk_instance),
            reinterpret_cast<VkPhysicalDevice>(vk_physical_device),
            vk_queue_infos
        );
    } catch (...)
    {
        return nullptr;
    }
}

void truvixx_gfx_device_destroy(const TruvixxGfxDeviceHandle device)
{
    delete device;
}

uint64_t truvixx_gfx_device_handle(const TruvixxGfxDeviceHandle device)
{
    if (!device)
        return 0;
    return reinterpret_cast<uint64_t>(device->impl.handle());
}

uint64_t truvixx_gfx_device_get_queue(
    const TruvixxGfxDeviceHandle device,
    const uint32_t queue_family_index,
    const uint32_t queue_index
)
{
    if (!device)
        return 0;

    try
    {
        VkQueue queue = device->impl.get_queue(queue_family_index, queue_index);
        return reinterpret_cast<uint64_t>(queue);
    } catch (...)
    {
        return 0;
    }
}

void truvixx_gfx_device_wait_idle(const TruvixxGfxDeviceHandle device)
{
    if (!device)
        return;

    try
    {
        device->impl.wait_idle();
    } catch (...)
    {
    }
}

void truvixx_gfx_device_set_object_debug_name(
    const TruvixxGfxDeviceHandle device,
    const uint64_t object_handle,
    const uint32_t object_type,
    const char* name
)
{
    if (!device || !name)
        return;

    try
    {
        device->impl.set_object_debug_name(
            object_handle,
            static_cast<VkObjectType>(object_type),
            name
        );
    } catch (...)
    {
    }
}

//=============================================================================
// GfxSwapchain C API 实现
//=============================================================================

TruvixxGfxSwapchainHandle truvixx_gfx_swapchain_create(
    const uint64_t vk_physical_device,
    const TruvixxGfxDeviceHandle device,
    const uint64_t vk_surface,
    const TruvixxGfxPresentMode present_mode,
    const TruvixxGfxSurfaceFormat surface_format,
    const uint32_t width,
    const uint32_t height,
    const uint64_t old_swapchain
)
{
    if (vk_physical_device == 0 || !device || vk_surface == 0)
        return nullptr;

    try
    {
        VkSurfaceFormatKHR vk_format{};
        vk_format.format = static_cast<VkFormat>(surface_format.format);
        vk_format.colorSpace = static_cast<VkColorSpaceKHR>(surface_format.color_space);

        VkExtent2D extent{ width, height };

        return new TruvixxGfxSwapchain(
            reinterpret_cast<VkPhysicalDevice>(vk_physical_device),
            &device->impl,
            reinterpret_cast<VkSurfaceKHR>(vk_surface),
            static_cast<VkPresentModeKHR>(present_mode),
            vk_format,
            extent,
            reinterpret_cast<VkSwapchainKHR>(old_swapchain)
        );
    } catch (...)
    {
        return nullptr;
    }
}

void truvixx_gfx_swapchain_destroy(const TruvixxGfxSwapchainHandle swapchain)
{
    delete swapchain;
}

uint64_t truvixx_gfx_swapchain_handle(const TruvixxGfxSwapchainHandle swapchain)
{
    if (!swapchain)
        return 0;
    return reinterpret_cast<uint64_t>(swapchain->impl.handle());
}

uint32_t truvixx_gfx_swapchain_extent_width(const TruvixxGfxSwapchainHandle swapchain)
{
    if (!swapchain)
        return 0;
    return swapchain->impl.extent().width;
}

uint32_t truvixx_gfx_swapchain_extent_height(const TruvixxGfxSwapchainHandle swapchain)
{
    if (!swapchain)
        return 0;
    return swapchain->impl.extent().height;
}

uint32_t truvixx_gfx_swapchain_format(const TruvixxGfxSwapchainHandle swapchain)
{
    if (!swapchain)
        return 0;
    return static_cast<uint32_t>(swapchain->impl.format());
}

uint32_t truvixx_gfx_swapchain_image_count(const TruvixxGfxSwapchainHandle swapchain)
{
    if (!swapchain)
        return 0;
    return static_cast<uint32_t>(swapchain->impl.images().size());
}

void truvixx_gfx_swapchain_get_images(
    const TruvixxGfxSwapchainHandle swapchain,
    uint64_t* out_images,
    const uint32_t count
)
{
    if (!swapchain || !out_images || count == 0)
        return;

    const auto& images = swapchain->impl.images();
    const uint32_t copy_count = std::min(count, static_cast<uint32_t>(images.size()));

    for (uint32_t i = 0; i < copy_count; ++i)
    {
        out_images[i] = reinterpret_cast<uint64_t>(images[i]);
    }
}

uint32_t truvixx_gfx_swapchain_current_image_index(const TruvixxGfxSwapchainHandle swapchain)
{
    if (!swapchain)
        return 0;
    return swapchain->impl.current_image_index();
}

void truvixx_gfx_swapchain_image_info(
    const TruvixxGfxSwapchainHandle swapchain,
    TruvixxGfxSwapchainImageInfo* out
)
{
    if (!swapchain || !out)
        return;

    const auto info = swapchain->impl.image_info();
    out->width = info.image_extent.width;
    out->height = info.image_extent.height;
    out->image_count = static_cast<uint32_t>(info.image_count);
    out->image_format = static_cast<uint32_t>(info.image_format);
}

uint32_t truvixx_gfx_swapchain_acquire_next_image(
    const TruvixxGfxSwapchainHandle swapchain,
    const uint64_t vk_semaphore,
    const uint64_t vk_fence,
    const uint64_t timeout
)
{
    if (!swapchain)
        return 1; // 需要重建

    try
    {
        bool need_rebuild = swapchain->impl.acquire_next_image(
            reinterpret_cast<VkSemaphore>(vk_semaphore),
            reinterpret_cast<VkFence>(vk_fence),
            timeout
        );
        return need_rebuild ? 1 : 0;
    } catch (...)
    {
        return 1;
    }
}

uint32_t truvixx_gfx_swapchain_present(
    const TruvixxGfxSwapchainHandle swapchain,
    const uint64_t vk_queue,
    const uint64_t* wait_semaphores,
    const uint32_t semaphore_count
)
{
    if (!swapchain || vk_queue == 0)
        return 1; // 需要重建

    try
    {
        std::vector<VkSemaphore> semaphores;
        if (wait_semaphores && semaphore_count > 0)
        {
            semaphores.reserve(semaphore_count);
            for (uint32_t i = 0; i < semaphore_count; ++i)
            {
                semaphores.push_back(reinterpret_cast<VkSemaphore>(wait_semaphores[i]));
            }
        }

        bool need_rebuild = swapchain->impl.present(
            reinterpret_cast<VkQueue>(vk_queue),
            semaphores
        );
        return need_rebuild ? 1 : 0;
    } catch (...)
    {
        return 1;
    }
}

//=============================================================================
// GfxCommandQueue C API 实现
//=============================================================================

TruvixxGfxCommandQueueHandle truvixx_gfx_command_queue_create(
    const uint64_t vk_queue,
    const TruvixxGfxQueueFamily* queue_family,
    const TruvixxGfxDeviceHandle device
)
{
    if (vk_queue == 0 || !queue_family || !device)
        return nullptr;

    try
    {
        truvixx::GfxQueueFamily family = convert_queue_family(*queue_family);

        return new TruvixxGfxCommandQueue(
            reinterpret_cast<VkQueue>(vk_queue),
            family,
            &device->impl
        );
    } catch (...)
    {
        return nullptr;
    }
}

void truvixx_gfx_command_queue_destroy(const TruvixxGfxCommandQueueHandle queue)
{
    delete queue;
}

uint64_t truvixx_gfx_command_queue_handle(const TruvixxGfxCommandQueueHandle queue)
{
    if (!queue)
        return 0;
    return reinterpret_cast<uint64_t>(queue->impl.handle());
}

void truvixx_gfx_command_queue_queue_family(
    const TruvixxGfxCommandQueueHandle queue,
    TruvixxGfxQueueFamily* out
)
{
    if (!queue || !out)
        return;

    fill_queue_family(out, queue->impl.queue_family());
}

void truvixx_gfx_command_queue_wait_idle(const TruvixxGfxCommandQueueHandle queue)
{
    if (!queue)
        return;

    try
    {
        queue->impl.waitIdle();
    } catch (...)
    {
    }
}

void truvixx_gfx_command_queue_submit(
    const TruvixxGfxCommandQueueHandle queue,
    const void* submit_infos,
    const uint32_t submit_count,
    const uint64_t vk_fence
)
{
    if (!queue || !submit_infos || submit_count == 0)
        return;

    try
    {
        const auto* infos = static_cast<const VkSubmitInfo2*>(submit_infos);
        std::vector<VkSubmitInfo2> infos_vec(infos, infos + submit_count);

        queue->impl.submit(infos_vec, reinterpret_cast<VkFence>(vk_fence));
    } catch (...)
    {
    }
}

void truvixx_gfx_command_queue_begin_label(
    const TruvixxGfxCommandQueueHandle queue,
    const char* label_name,
    const float r,
    const float g,
    const float b,
    const float a
)
{
    if (!queue || !label_name)
        return;

    try
    {
        queue->impl.begin_label(label_name, r, g, b, a);
    } catch (...)
    {
    }
}

void truvixx_gfx_command_queue_end_label(const TruvixxGfxCommandQueueHandle queue)
{
    if (!queue)
        return;

    try
    {
        queue->impl.end_label();
    } catch (...)
    {
    }
}

void truvixx_gfx_command_queue_insert_label(
    const TruvixxGfxCommandQueueHandle queue,
    const char* label_name,
    const float r,
    const float g,
    const float b,
    const float a
)
{
    if (!queue || !label_name)
        return;

    try
    {
        queue->impl.insert_label(label_name, r, g, b, a);
    } catch (...)
    {
    }
}