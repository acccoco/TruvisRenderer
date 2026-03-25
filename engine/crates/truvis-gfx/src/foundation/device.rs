use ash::vk;
use itertools::Itertools;
use std::cell::Cell;
use std::{
    ffi::{CStr, CString},
    ops::Deref,
};

use crate::{foundation::debug_messenger::DebugType, utilities::descriptor_cursor::GfxWriteDescriptorSet};

/// Vulkan 逻辑设备封装
///
/// 包含核心设备 API 以及各种扩展的函数指针（如动态渲染、光线追踪、调试工具等）。
/// 这些函数指针在应用生命周期中保持不变，可以安全共享。
///
/// # 扩展支持
/// - Dynamic Rendering (KHR)
/// - Acceleration Structure (KHR)
/// - Ray Tracing Pipeline (KHR)
/// - Debug Utils (EXT)
/// - Swapchain (KHR)
pub struct GfxDevice {
    /// 核心 Vulkan 设备 API
    pub(crate) device: ash::Device,
    /// 动态渲染扩展 API
    pub(crate) dynamic_rendering: ash::khr::dynamic_rendering::Device,
    /// 加速结构扩展 API  
    pub(crate) acceleration_structure: ash::khr::acceleration_structure::Device,
    /// 光线追踪管线扩展 API
    pub(crate) ray_tracing_pipeline: ash::khr::ray_tracing_pipeline::Device,
    /// 调试工具扩展 API
    pub(crate) debug_utils: ash::ext::debug_utils::Device,
    /// 交换链扩展 API
    pub(crate) swapchain: ash::khr::swapchain::Device,
    /// 推送描述符扩展 API
    pub(crate) push_descriptor: ash::khr::push_descriptor::Device,

    #[cfg(debug_assertions)]
    destroyed: Cell<bool>,
}

// 构造与销毁
impl GfxDevice {
    pub fn new(
        instance: &ash::Instance,
        pdevice: vk::PhysicalDevice,
        queue_create_info: &[vk::DeviceQueueCreateInfo],
    ) -> Self {
        let _span = tracy_client::span!("GfxDevice::new");

        // device 所需的所有 extension
        let device_exts = Self::basic_device_exts().iter().map(|e| e.as_ptr()).collect_vec();
        let mut exts_str = String::new();
        for ext in &device_exts {
            exts_str.push_str(&format!("\n\t{:?}", unsafe { CStr::from_ptr(*ext) }));
        }
        log::info!("device exts: {}", exts_str);

        // device 所需的所有 features
        let mut all_features = vk::PhysicalDeviceFeatures2::default().features(Self::physical_device_basic_features());
        let mut physical_device_ext_features = Self::physical_device_extra_features();
        unsafe {
            physical_device_ext_features.iter_mut().for_each(|f| {
                let ptr = <*mut dyn vk::ExtendsPhysicalDeviceFeatures2>::cast::<vk::BaseOutStructure>(f.as_mut());
                (*ptr).p_next = all_features.p_next as _;
                all_features.p_next = ptr as _;
            });
        }

        let device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(queue_create_info)
            .enabled_extension_names(&device_exts)
            .push_next(&mut all_features);

        let device = unsafe { instance.create_device(pdevice, &device_create_info, None).unwrap() };

        let vk_dynamic_render_pf = ash::khr::dynamic_rendering::Device::new(instance, &device);
        let vk_acceleration_struct_pf = ash::khr::acceleration_structure::Device::new(instance, &device);
        let vk_rt_pipeline_pf = ash::khr::ray_tracing_pipeline::Device::new(instance, &device);
        let vk_debug_utils_device = ash::ext::debug_utils::Device::new(instance, &device);
        let vk_swapchain = ash::khr::swapchain::Device::new(instance, &device);
        let vk_push_descriptor = ash::khr::push_descriptor::Device::new(instance, &device);

        Self {
            device: device.clone(),
            dynamic_rendering: vk_dynamic_render_pf,
            acceleration_structure: vk_acceleration_struct_pf,
            ray_tracing_pipeline: vk_rt_pipeline_pf,
            debug_utils: vk_debug_utils_device,
            swapchain: vk_swapchain,
            push_descriptor: vk_push_descriptor,

            #[cfg(debug_assertions)]
            destroyed: Cell::new(false),
        }
    }

    pub fn destroy(&self) {
        log::info!("destroying device");

        #[cfg(debug_assertions)]
        self.destroyed.set(true);

        unsafe {
            self.device.destroy_device(None);
        }
    }
}

// 创建过程的辅助函数
impl GfxDevice {
    /// 必要的 physical device core features
    fn physical_device_basic_features() -> vk::PhysicalDeviceFeatures {
        vk::PhysicalDeviceFeatures::default()
            .sampler_anisotropy(true)
            .fragment_stores_and_atomics(true)
            .independent_blend(true)
            .shader_int64(true) // 用于 buffer device address
    }

    /// 必要的 physical device extension features
    fn physical_device_extra_features() -> Vec<Box<dyn vk::ExtendsPhysicalDeviceFeatures2>> {
        vec![
            Box::new(vk::PhysicalDeviceDynamicRenderingFeatures::default().dynamic_rendering(true)),
            Box::new(vk::PhysicalDeviceBufferDeviceAddressFeatures::default().buffer_device_address(true)),
            Box::new(vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::default().ray_tracing_pipeline(true)),
            Box::new(vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default().acceleration_structure(true)),
            Box::new(vk::PhysicalDeviceRayQueryFeaturesKHR::default().ray_query(true)),
            Box::new(
                vk::PhysicalDeviceRayTracingInvocationReorderFeaturesNV::default().ray_tracing_invocation_reorder(true),
            ),
            Box::new(vk::PhysicalDeviceHostQueryResetFeatures::default().host_query_reset(true)),
            Box::new(vk::PhysicalDeviceSynchronization2Features::default().synchronization2(true)),
            Box::new(vk::PhysicalDeviceTimelineSemaphoreFeatures::default().timeline_semaphore(true)),
            Box::new(
                vk::PhysicalDeviceDescriptorIndexingFeatures::default()
                    .descriptor_binding_partially_bound(true) // 即使一些 descriptor 是 invalid
                    .runtime_descriptor_array(true)
                    .descriptor_binding_sampled_image_update_after_bind(true)
                    .descriptor_binding_storage_image_update_after_bind(true)
                    .descriptor_binding_update_unused_while_pending(true)
                    .descriptor_binding_variable_descriptor_count(true),
            ),
            Box::new(vk::PhysicalDeviceShaderDrawParametersFeatures::default().shader_draw_parameters(true)),
            // 让 UBO 支持 std430 或者 scalar layout
            Box::new(
                vk::PhysicalDeviceUniformBufferStandardLayoutFeatures::default().uniform_buffer_standard_layout(true),
            ),
        ]
    }

    /// 必要的 device extensions
    fn basic_device_exts() -> Vec<&'static CStr> {
        let mut exts = vec![];

        // swapchain
        exts.push(ash::khr::swapchain::NAME);

        // dynamic rendering
        exts.append(&mut vec![
            // 已经提升到 core-1.2.0
            // ash::khr::depth_stencil_resolve::NAME,
            // ash::khr::create_renderpass2::NAME,
            ash::khr::dynamic_rendering::NAME,
        ]);

        // RayTracing 相关的
        exts.append(&mut vec![
            ash::khr::acceleration_structure::NAME, // 主要的 ext
            // 已经提升到 core-1.2.0
            // ash::ext::descriptor_indexing::NAME,
            // ash::khr::buffer_device_address::NAME,
            // ash::khr::spirv_1_4::NAME,
            // ash::khr::shader_float_controls::NAME,
            ash::khr::ray_tracing_pipeline::NAME, // 主要的 ext
            ash::khr::ray_query::NAME,            // RayQuery 支持
            ash::khr::deferred_host_operations::NAME,
        ]);

        // Shader Execution Reordering (NVIDIA)
        exts.push(ash::nv::ray_tracing_invocation_reorder::NAME);

        // push descriptor
        exts.push(ash::khr::push_descriptor::NAME);

        exts
    }
}

// getters
impl GfxDevice {
    #[inline]
    pub fn vk_handle(&self) -> vk::Device {
        self.device.handle()
    }
    #[inline]
    pub fn dynamic_rendering(&self) -> &ash::khr::dynamic_rendering::Device {
        &self.dynamic_rendering
    }
    #[inline]
    pub fn acceleration_structure(&self) -> &ash::khr::acceleration_structure::Device {
        &self.acceleration_structure
    }
    #[inline]
    pub fn ray_tracing_pipeline(&self) -> &ash::khr::ray_tracing_pipeline::Device {
        &self.ray_tracing_pipeline
    }
    #[inline]
    pub fn debug_utils(&self) -> &ash::ext::debug_utils::Device {
        &self.debug_utils
    }
    #[inline]
    pub fn swapchain(&self) -> &ash::khr::swapchain::Device {
        &self.swapchain
    }
}

// tools
impl GfxDevice {
    #[inline]
    pub fn write_descriptor_sets(&self, writes: &[GfxWriteDescriptorSet]) {
        GfxWriteDescriptorSet::with_writes(writes, |writes| unsafe {
            self.device.update_descriptor_sets(writes, &[]);
        })
    }

    #[inline]
    pub fn set_object_debug_name<T: vk::Handle + Copy>(&self, handle: T, name: impl AsRef<str>) {
        let name = CString::new(name.as_ref()).unwrap();
        unsafe {
            self.debug_utils
                .set_debug_utils_object_name(
                    &vk::DebugUtilsObjectNameInfoEXT::default().object_name(name.as_c_str()).object_handle(handle),
                )
                .unwrap();
        }
    }

    pub fn set_debug_name<T: DebugType>(&self, handle: &T, name: impl AsRef<str>) {
        let debug_name = format!("{}::{}", T::debug_type_name(), name.as_ref());
        let debug_name = CString::new(debug_name.as_str()).unwrap();
        unsafe {
            self.debug_utils
                .set_debug_utils_object_name(
                    &vk::DebugUtilsObjectNameInfoEXT::default()
                        .object_name(debug_name.as_c_str())
                        .object_handle(handle.vk_handle()),
                )
                .unwrap();
        }
    }

    #[inline]
    pub fn wait_idle(&self) {
        unsafe {
            self.device.device_wait_idle().unwrap();
        }
    }
}

impl Deref for GfxDevice {
    type Target = ash::Device;
    fn deref(&self) -> &Self::Target {
        &self.device
    }
}
impl Drop for GfxDevice {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        debug_assert!(self.destroyed.get(), "GfxDevice must be destroyed before being dropped.");
    }
}
impl DebugType for GfxDevice {
    fn debug_type_name() -> &'static str {
        "GfxDevice"
    }
    fn vk_handle(&self) -> impl vk::Handle {
        self.device.handle()
    }
}
