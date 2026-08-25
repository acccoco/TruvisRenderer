use std::ffi::CStr;
use std::rc::Rc;

use ash::vk;
use truvis_streamline_binding::StreamlineRuntime;

use crate::gfx_core::{GfxCore, VulkanEntrySource};
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

/// 只暴露 Vulkan device 能力的借用视图。
#[derive(Clone, Copy)]
pub struct GfxDeviceCtx<'a> {
    device: &'a Rc<GfxDevice>,
}

impl<'a> GfxDeviceCtx<'a> {
    #[inline]
    pub fn device(self) -> &'a GfxDevice {
        self.device
    }

    #[inline]
    pub(crate) fn device_rc(self) -> Rc<GfxDevice> {
        self.device.clone()
    }
}

/// 资源创建/销毁所需的 device + VMA allocator 借用视图。
#[derive(Clone, Copy)]
pub struct GfxResourceCtx<'a> {
    device: &'a Rc<GfxDevice>,
    allocator: &'a VMemAllocator,
}

impl<'a> GfxResourceCtx<'a> {
    #[inline]
    pub fn device(self) -> &'a GfxDevice {
        self.device
    }

    #[inline]
    pub fn allocator(self) -> &'a VMemAllocator {
        self.allocator
    }
}

/// 队列提交和队列标签所需的借用视图。
#[derive(Clone, Copy)]
pub struct GfxQueueCtx<'a> {
    gfx_queue: &'a GfxCommandQueue,
    transfer_queue: &'a GfxCommandQueue,
}

impl<'a> GfxQueueCtx<'a> {
    #[inline]
    pub fn gfx_queue(self) -> &'a GfxCommandQueue {
        self.gfx_queue
    }

    #[inline]
    pub fn transfer_queue(self) -> &'a GfxCommandQueue {
        self.transfer_queue
    }
}

/// 只读设备信息与格式查询能力。
#[derive(Clone, Copy)]
pub struct GfxDeviceInfoCtx<'a> {
    instance: &'a GfxInstance,
    physical_device: &'a GfxPhysicalDevice,
}

impl<'a> GfxDeviceInfoCtx<'a> {
    #[inline]
    pub fn instance(self) -> &'a GfxInstance {
        self.instance
    }

    #[inline]
    pub fn physical_device(self) -> &'a GfxPhysicalDevice {
        self.physical_device
    }

    #[inline]
    pub fn gfx_queue_family(self) -> GfxQueueFamily {
        self.physical_device.gfx_queue_family.clone()
    }

    #[inline]
    pub fn compute_queue_family(self) -> GfxQueueFamily {
        self.physical_device.compute_queue_family.as_ref().unwrap().clone()
    }

    #[inline]
    pub fn transfer_queue_family(self) -> GfxQueueFamily {
        self.physical_device.transfer_queue_family.as_ref().unwrap().clone()
    }

    #[inline]
    pub fn min_ubo_offset_align(self) -> vk::DeviceSize {
        self.physical_device.basic_props.limits.min_uniform_buffer_offset_alignment
    }

    #[inline]
    pub fn rt_pipeline_props(self) -> &'a vk::PhysicalDeviceRayTracingPipelinePropertiesKHR<'static> {
        &self.physical_device.rt_pipeline_props
    }

    /// 根据给定的格式，返回支持的格式。
    pub fn find_supported_format(
        self,
        candidates: &[vk::Format],
        tiling: vk::ImageTiling,
        features: vk::FormatFeatureFlags,
    ) -> Vec<vk::Format> {
        candidates
            .iter()
            .filter(|f| {
                let props = unsafe {
                    self.instance
                        .ash_instance
                        .get_physical_device_format_properties(self.physical_device.vk_handle, **f)
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
}

/// one-time command 执行所需的借用视图。
#[derive(Clone, Copy)]
pub struct GfxImmediateCtx<'a> {
    device: &'a Rc<GfxDevice>,
    queue: &'a GfxCommandQueue,
    command_pool: &'a GfxCommandPool,
}

impl<'a> GfxImmediateCtx<'a> {
    #[inline]
    pub fn device(self) -> &'a GfxDevice {
        self.device
    }

    #[inline]
    pub fn queue(self) -> &'a GfxCommandQueue {
        self.queue
    }

    #[inline]
    pub fn command_pool(self) -> &'a GfxCommandPool {
        self.command_pool
    }

    #[inline]
    pub(crate) fn device_rc(self) -> Rc<GfxDevice> {
        self.device.clone()
    }
}

/// WSI surface/swapchain 操作所需的借用视图。
#[derive(Clone, Copy)]
pub struct GfxSurfaceCtx<'a> {
    core: &'a GfxCore,
}

impl<'a> GfxSurfaceCtx<'a> {
    #[inline]
    pub(crate) fn core(self) -> &'a GfxCore {
        self.core
    }

    #[inline]
    pub fn device(self) -> &'a GfxDevice {
        &self.core.gfx_device
    }

    #[inline]
    pub fn physical_device(self) -> &'a GfxPhysicalDevice {
        &self.core.physical_device
    }
}

/// Vulkan 图形上下文 root owner。
///
/// 管理所有 Vulkan 核心资源，包括实例、设备、队列、内存分配器等。
/// 子资源创建、使用和销毁应通过 typed Ctx 显式接收所需能力。
pub struct Gfx {
    pub(crate) gfx_core: GfxCore,
    pub(crate) vm_allocator: VMemAllocator,

    /// 临时的 graphics command pool，主要用于临时的命令缓冲区
    pub(crate) temp_graphics_command_pool: GfxCommandPool,

    /// Streamline 是进程级 runtime。Gfx 负责让它先于 Vulkan root 初始化，并在 Vulkan root 销毁前关闭。
    streamline_runtime: Option<StreamlineRuntime>,
}

// 创建与销毁
impl Gfx {
    // region init 相关
    const ENGINE_NAME: &'static str = "DruvisIII";

    /// 创建生产路径使用的 Vulkan root owner。
    ///
    /// 默认路径先初始化进程级 Streamline runtime，再通过 `sl.interposer.dll` 创建 Vulkan entry。
    /// 这样后续 Vulkan instance/device 和调用链都处在 Streamline interposer 可见范围内。
    pub fn new(app_name: String, instance_extra_exts: Vec<&'static CStr>) -> Self {
        let streamline_runtime = Self::init_streamline_runtime();
        let vulkan_loader_path = streamline_runtime.vulkan_loader_path();
        log::info!(
            "Gfx Streamline enabled: plugin_dir={}, vulkan_loader={}, log_dir={}",
            streamline_runtime.plugin_dir().display(),
            vulkan_loader_path.display(),
            streamline_runtime.log_dir().display()
        );

        Self::new_internal(
            app_name,
            instance_extra_exts,
            VulkanEntrySource::DllPath(vulkan_loader_path),
            Some(streamline_runtime),
        )
    }

    /// 使用显式 Vulkan loader 创建 root owner。
    ///
    /// 该入口只负责按传入的 `VulkanEntrySource` 构造 Vulkan root，不自动初始化或持有
    /// Streamline runtime。测试、工具或未来特殊启动路径如果需要 Streamline，必须在外层
    /// 自己保证 `slInit -> Vulkan root -> child resources -> slShutdown` 的顺序。
    pub fn new_with_entry_source(
        app_name: String,
        instance_extra_exts: Vec<&'static CStr>,
        entry_source: VulkanEntrySource,
    ) -> Self {
        Self::new_internal(app_name, instance_extra_exts, entry_source, None)
    }

    fn init_streamline_runtime() -> StreamlineRuntime {
        StreamlineRuntime::init_default().unwrap_or_else(|err| {
            log::error!("Gfx failed to initialize Streamline before Vulkan startup: {}", err);
            panic!("Gfx failed to initialize Streamline before Vulkan startup: {err}");
        })
    }

    fn new_internal(
        app_name: String,
        instance_extra_exts: Vec<&'static CStr>,
        entry_source: VulkanEntrySource,
        streamline_runtime: Option<StreamlineRuntime>,
    ) -> Self {
        let _span = tracy_client::span!("Gfx::new");
        match &entry_source {
            VulkanEntrySource::System => {
                log::info!("Gfx loading Vulkan entry from system loader.");
            }
            VulkanEntrySource::DllPath(path) => {
                log::info!("Gfx loading Vulkan entry from DLL: {}", path.display());
            }
        }

        let gfx_core =
            GfxCore::new_with_entry_source(app_name, Self::ENGINE_NAME.to_string(), instance_extra_exts, entry_source);

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
            streamline_runtime,
        }
    }

    /// 销毁 Gfx root owner。调用者必须先显式释放所有子 GPU 资源。
    pub fn destroy(self) {
        let Self {
            gfx_core,
            vm_allocator,
            temp_graphics_command_pool,
            streamline_runtime,
        } = self;

        // Gfx 自己的 command pool / allocator 仍是 Vulkan device child，先释放它们；
        // Streamline SDK 要求 slShutdown 发生在 VkDevice / VkInstance 等 root 销毁前。
        temp_graphics_command_pool.destroy_internal(&gfx_core.gfx_device);
        vm_allocator.destroy();
        if streamline_runtime.is_some() {
            log::info!("Gfx shutting down Streamline before Vulkan root destruction.");
        }
        drop(streamline_runtime);
        gfx_core.destroy();
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
    /// 注：这个值一定是 2 的幂
    #[inline]
    pub fn min_ubo_offset_align(&self) -> vk::DeviceSize {
        self.gfx_core.physical_device.basic_props.limits.min_uniform_buffer_offset_alignment
    }

    #[inline]
    pub fn rt_pipeline_props(&self) -> &vk::PhysicalDeviceRayTracingPipelinePropertiesKHR<'_> {
        &self.gfx_core.physical_device.rt_pipeline_props
    }

    #[inline]
    pub fn device_ctx(&self) -> GfxDeviceCtx<'_> {
        GfxDeviceCtx {
            device: &self.gfx_core.gfx_device,
        }
    }

    #[inline]
    pub fn resource_ctx(&self) -> GfxResourceCtx<'_> {
        GfxResourceCtx {
            device: &self.gfx_core.gfx_device,
            allocator: &self.vm_allocator,
        }
    }

    #[inline]
    pub fn queue_ctx(&self) -> GfxQueueCtx<'_> {
        GfxQueueCtx {
            gfx_queue: &self.gfx_core.gfx_queue,
            transfer_queue: &self.gfx_core.transfer_queue,
        }
    }

    #[inline]
    pub fn device_info_ctx(&self) -> GfxDeviceInfoCtx<'_> {
        GfxDeviceInfoCtx {
            instance: &self.gfx_core.instance,
            physical_device: &self.gfx_core.physical_device,
        }
    }

    #[inline]
    pub fn immediate_ctx(&self) -> GfxImmediateCtx<'_> {
        GfxImmediateCtx {
            device: &self.gfx_core.gfx_device,
            queue: &self.gfx_core.gfx_queue,
            command_pool: &self.temp_graphics_command_pool,
        }
    }

    #[inline]
    pub fn surface_ctx(&self) -> GfxSurfaceCtx<'_> {
        GfxSurfaceCtx { core: &self.gfx_core }
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
        self.immediate_ctx().one_time_exec(func, name)
    }

    pub fn wait_idel(&self) {
        self.device_ctx().device().wait_idle();
    }
}

impl GfxImmediateCtx<'_> {
    /// 立即执行某个 command，并同步等待执行结果。
    pub fn one_time_exec<F, R>(self, func: F, name: impl AsRef<str>) -> R
    where
        F: FnOnce(&GfxCommandBuffer) -> R,
    {
        let command_buffer = GfxCommandBuffer::new_with_device(
            self.device_rc(),
            self.command_pool,
            &format!("one-time-{}", name.as_ref()),
        );

        command_buffer.begin(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, name.as_ref());
        let result = func(&command_buffer);
        command_buffer.end();

        let command_buffer_clone = command_buffer.clone();
        self.queue.submit(vec![GfxSubmitInfo::new(&[command_buffer_clone])], None);
        self.queue.wait_idle();
        unsafe {
            self.device.free_command_buffers(self.command_pool.handle(), &[command_buffer.vk_handle()]);
        }

        result
    }
}
