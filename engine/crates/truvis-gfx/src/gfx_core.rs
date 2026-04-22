use std::ffi::CStr;
use std::rc::Rc;

use ash::vk;

use crate::{
    commands::command_queue::GfxCommandQueue,
    foundation::{
        debug_messenger::GfxDebugMsger, device::GfxDevice, instance::GfxInstance, physical_device::GfxPhysicalDevice,
    },
};

/// Vulkan 核心组件集合
///
/// 包含 Entry、Instance、PhysicalDevice、Device、Queue 等 Vulkan 基础对象。
/// 不包含内存分配器等高层抽象，仅提供 Vulkan 原生功能。
pub struct GfxCore {
    /// Vulkan 库入口（加载 vulkan-1.dll）
    ///
    /// 在 drop 之后会卸载 DLL，需要确保该字段最后 drop
    pub(crate) vk_entry: ash::Entry,

    pub(crate) instance: GfxInstance,
    pub(crate) physical_device: GfxPhysicalDevice,

    /// Vulkan 设备函数指针集合（Rc 共享）
    ///
    /// 使用 `Rc` 的原因：
    /// 1. 多个组件需要共享相同的设备函数指针（Queue、CommandBuffer 等）
    /// 2. 函数指针本身轻量，共享比传递更高效
    /// 3. 设备生命周期需要精确控制，`Rc` 确保在所有引用者销毁前设备不被销毁
    pub(crate) gfx_device: Rc<GfxDevice>,

    pub(crate) debug_utils: GfxDebugMsger,

    pub(crate) gfx_queue: GfxCommandQueue,
    pub(crate) transfer_queue: GfxCommandQueue,
}

// 创建与销毁
impl GfxCore {
    pub fn new(app_name: String, engine_name: String, instance_extra_exts: Vec<&'static CStr>) -> Self {
        let _span = tracy_client::span!("GfxCore::new");

        let vk_pf = unsafe { ash::Entry::load() }.expect("Failed to load vulkan entry");
        let instance = GfxInstance::new(&vk_pf, app_name, engine_name, instance_extra_exts);
        let physical_device = GfxPhysicalDevice::new_descrete_physical_device(instance.ash_instance());

        // Nvidia 使用的是 Unified Scheduler，因此 Graphics 和 Compute 并没法做到真正的并行
        // Graphics 和 Compute 会争夺 SM，L2 以及显存
        // 驱动层给出了专用的 compute queue family，但是底层硬件资源依然是共享的
        // Transfer(DMA) 可以做到部分并行，不过为了简化设计，仍然然使用同一个 queue family

        // 尝试从 Graphics Queue Family 中申请两个队列，一个用于 Graphics，一个用于 Transfer
        let gfx_family_idx = physical_device.gfx_queue_family.queue_family_index;
        let max_queues = physical_device.gfx_queue_family.queue_count;

        if max_queues < 2 {
            panic!(
                "Graphics queue family has {} queues, but at least 2 are required for separate Graphics and Transfer queues.",
                max_queues
            );
        }

        let request_queue_count = 2;
        let priorities = vec![1.0; request_queue_count as usize];

        let queue_create_infos =
            [vk::DeviceQueueCreateInfo::default().queue_family_index(gfx_family_idx).queue_priorities(&priorities)];

        let device = Rc::new(GfxDevice::new(&instance.ash_instance, physical_device.vk_handle, &queue_create_infos));

        let gfx_queue = GfxCommandQueue {
            vk_queue: unsafe { device.get_device_queue(gfx_family_idx, 0) },
            queue_family: physical_device.gfx_queue_family.clone(),
            gfx_device: device.clone(),
        };

        let transfer_queue = GfxCommandQueue {
            vk_queue: unsafe { device.get_device_queue(gfx_family_idx, 1) },
            queue_family: physical_device.gfx_queue_family.clone(),
            gfx_device: device.clone(),
        };

        let debug_utils = GfxDebugMsger::new(&vk_pf, &instance.ash_instance);

        log::info!("gfx queue's queue family:\n{:#?}", gfx_queue.queue_family);
        log::info!("transfer queue's queue family:\n{:#?}", transfer_queue.queue_family);

        // 在 device 以及 debug_utils 之前创建的 vk::Handle
        {
            device.set_object_debug_name(instance.vk_instance(), "GfxInstance");
            device.set_object_debug_name(physical_device.vk_handle, "GfxPhysicalDevice");

            device.set_object_debug_name(device.vk_handle(), "GfxDevice");
            device.set_object_debug_name(gfx_queue.vk_queue, "CommandQueue-gfx");
            device.set_object_debug_name(transfer_queue.vk_queue, "CommandQueue-transfer");
        }

        Self {
            vk_entry: vk_pf,
            instance,
            physical_device,
            gfx_device: device,
            debug_utils,
            gfx_queue,
            transfer_queue,
        }
    }

    pub fn destroy(self) {
        self.debug_utils.destroy();
        self.gfx_device.destroy();
        self.physical_device.destroy();
        self.instance.destroy();
    }
}
