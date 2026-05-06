use std::ffi::CStr;

use ash::vk;

use crate::gfx_core::GfxCore;
use crate::{
    commands::{
        command_buffer::GfxCommandBuffer,
        command_pool::GfxCommandPool,
        command_queue::{GfxCommandQueue, GfxQueueFamily},
        submit_info::GfxSubmitInfo,
    },
    foundation::{
        device::GfxDevice, instance::GfxInstance, physical_device::GfxPhysicalDevice, vmem_allocator::VMemAllocator,
    },
};

/// Vulkan 图形上下文单例
///
/// 管理所有 Vulkan 核心资源，包括实例、设备、队列、内存分配器等。
/// 采用单例模式简化参数传递和生命周期管理，仅适用于单线程环境。
///
/// # 初始化流程
/// ```ignore
/// Gfx::init("MyApp".to_string(), extra_extensions);
/// let device = Gfx::get().gfx_device();
/// // 使用...
/// Gfx::destroy();
/// ```
pub struct Gfx {
    pub(crate) gfx_core: GfxCore,
    pub(crate) vm_allocator: VMemAllocator,

    /// 临时的 graphics command pool，主要用于临时的命令缓冲区
    pub(crate) temp_graphics_command_pool: GfxCommandPool,
}

// 创建与销毁
impl Gfx {
    // region init 相关
    const ENGINE_NAME: &'static str = "DruvisIII";

    fn new(app_name: String, instance_extra_exts: Vec<&'static CStr>) -> Self {
        let _span = tracy_client::span!("Gfx::new");

        let gfx_core = GfxCore::new(app_name, Self::ENGINE_NAME.to_string(), instance_extra_exts);

        // 注意：在初始化过程中，我们需要使用传统的参数传递方式
        // 因为 RenderContext 单例还没有被初始化
        let gfx_command_pool = GfxCommandPool::new_internal(
            gfx_core.gfx_device.clone(),
            gfx_core.physical_device.gfx_queue_family.clone(),
            vk::CommandPoolCreateFlags::empty(),
            "render_context-graphics",
        );

        let allocator = VMemAllocator::new(
            &gfx_core.instance.ash_instance,
            gfx_core.physical_device.vk_handle,
            &gfx_core.gfx_device,
        );

        Self {
            gfx_core,
            vm_allocator: allocator,
            temp_graphics_command_pool: gfx_command_pool,
        }
    }
}

// 注意：此静态变量仅用于单线程环境，符合项目要求
static mut G_GFX: Option<Gfx> = None;

// 单例模式
// - RenderContext 自身的生命周期管理比较简单，因此适合使用单例模式
// - 让代码变得简单，不再需要考虑复杂的借用规则
// - 其他类的类型签名也会变得更简单
impl Gfx {
    /// 获取单例实例
    ///
    /// # Panics
    /// 如果 RenderContext 还未初始化，此方法会 panic
    ///
    /// # Safety
    /// 此方法仅在单线程环境下安全
    #[inline]
    pub fn get() -> &'static Gfx {
        unsafe {
            // 使用 addr_of! 避免直接对 static mut 创建引用，编译器不允许这种行为
            let ptr = std::ptr::addr_of!(G_GFX);
            (*ptr).as_ref().expect("RenderContext not initialized. Call RenderContext::init() first.")
        }
    }

    /// 初始化 RenderContext 单例
    ///
    /// # 参数
    /// - `app_name`: 应用程序名称
    /// - `instance_extra_exts`: 额外的 Vulkan 实例扩展
    ///
    /// # Panics
    /// 如果 RenderContext 已经被初始化，此方法会 panic
    ///
    /// # Safety
    /// 此方法仅在单线程环境下安全
    pub fn init(app_name: String, instance_extra_exts: Vec<&'static CStr>) {
        unsafe {
            // 使用 addr_of_mut! 避免直接对 static mut 创建可变引用
            let ptr = std::ptr::addr_of_mut!(G_GFX);
            assert!((*ptr).is_none(), "RenderContext already initialized");
            *ptr = Some(Self::new(app_name, instance_extra_exts));
        }
    }

    /// 销毁 RenderContext 单例
    ///
    /// # Safety
    /// 调用此方法后，不应再使用 RenderContext::get()
    /// 此方法仅在单线程环境下安全
    pub fn destroy() {
        unsafe {
            // 使用 addr_of_mut! 避免直接对 static mut 创建可变引用
            let ptr = std::ptr::addr_of_mut!(G_GFX);
            let context = (*ptr).take().expect("RenderContext not initialized");

            context.vm_allocator.destroy();
            context.temp_graphics_command_pool.destroy_internal(&context.gfx_core.gfx_device);
            context.gfx_core.destroy();
        }
    }
}

// 访问器
impl Gfx {
    #[inline]
    pub fn vk_core(&self) -> &GfxCore {
        &self.gfx_core
    }

    #[inline]
    pub fn instance(&self) -> &GfxInstance {
        &self.gfx_core.instance
    }

    #[inline]
    pub fn gfx_device(&self) -> &GfxDevice {
        &self.gfx_core.gfx_device
    }

    #[inline]
    pub fn allocator(&self) -> &VMemAllocator {
        &self.vm_allocator
    }

    #[inline]
    pub fn physical_device(&self) -> &GfxPhysicalDevice {
        &self.gfx_core.physical_device
    }

    #[inline]
    pub fn gfx_queue_family(&self) -> GfxQueueFamily {
        self.gfx_core.physical_device.gfx_queue_family.clone()
    }

    #[inline]
    pub fn compute_queue_family(&self) -> GfxQueueFamily {
        self.gfx_core.physical_device.compute_queue_family.as_ref().unwrap().clone()
    }

    #[inline]
    pub fn transfer_queue_family(&self) -> GfxQueueFamily {
        self.gfx_core.physical_device.transfer_queue_family.as_ref().unwrap().clone()
    }

    #[inline]
    pub fn gfx_queue(&self) -> &GfxCommandQueue {
        &self.gfx_core.gfx_queue
    }

    #[inline]
    pub fn transfer_queue(&self) -> &GfxCommandQueue {
        &self.gfx_core.transfer_queue
    }

    /// 当 uniform buffer 的 descriptor 在更新时，其 offset 必须是这个值的整数倍
    ///
    /// 注：这个值一定是 power of 2
    #[inline]
    pub fn min_ubo_offset_align(&self) -> vk::DeviceSize {
        self.gfx_core.physical_device.basic_props.limits.min_uniform_buffer_offset_alignment
    }

    #[inline]
    pub fn rt_pipeline_props(&self) -> &vk::PhysicalDeviceRayTracingPipelinePropertiesKHR<'_> {
        &self.gfx_core.physical_device.rt_pipeline_props
    }
}

// 工具函数
impl Gfx {
    /// 根据给定的格式，返回支持的格式
    pub fn find_supported_format(
        &self,
        candidates: &[vk::Format],
        tiling: vk::ImageTiling,
        features: vk::FormatFeatureFlags,
    ) -> Vec<vk::Format> {
        candidates
            .iter()
            .filter(|f| {
                let props = unsafe {
                    self.instance()
                        .ash_instance
                        .get_physical_device_format_properties(self.physical_device().vk_handle, **f)
                };
                match tiling {
                    vk::ImageTiling::LINEAR => props.linear_tiling_features.contains(features),
                    vk::ImageTiling::OPTIMAL => props.optimal_tiling_features.contains(features),
                    _ => panic!("not supported tiling."),
                }
            })
            .copied()
            .collect()
    }

    /// 立即执行某个 command，并同步等待执行结果
    pub fn one_time_exec<F, R>(&self, func: F, name: impl AsRef<str>) -> R
    where
        F: FnOnce(&GfxCommandBuffer) -> R,
    {
        let command_buffer =
            GfxCommandBuffer::new(&self.temp_graphics_command_pool, &format!("one-time-{}", name.as_ref()));

        command_buffer.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, name.as_ref());
        let result = func(&command_buffer);
        command_buffer.end();

        let command_buffer_clone = command_buffer.clone();
        self.gfx_queue().submit(vec![GfxSubmitInfo::new(&[command_buffer_clone])], None);
        self.gfx_queue().wait_idle();
        unsafe {
            self.gfx_device()
                .free_command_buffers(self.temp_graphics_command_pool.handle(), &[command_buffer.vk_handle()]);
        }

        result
    }

    pub fn wait_idel(&self) {
        unsafe {
            self.gfx_device().device_wait_idle().unwrap();
        }
    }
}
