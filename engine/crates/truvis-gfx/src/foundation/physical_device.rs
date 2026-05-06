use std::{ffi::CStr, ptr::null_mut};

use ash::vk;
use itertools::Itertools;

use crate::{commands::command_queue::GfxQueueFamily, foundation::debug_messenger::DebugType};

/// 表示一张物理显卡
pub struct GfxPhysicalDevice {
    pub(crate) vk_handle: vk::PhysicalDevice,

    /// 当前 gpu 支持的 features
    pub(crate) _features: vk::PhysicalDeviceFeatures,

    /// 当前 gpu 支持的 device extensions
    pub(crate) _device_extensions: Vec<vk::ExtensionProperties>,

    /// 当前 gpu 的基础属性
    pub(crate) basic_props: vk::PhysicalDeviceProperties,

    /// 当前 gpu 的 ray tracing 属性
    pub(crate) rt_pipeline_props: vk::PhysicalDeviceRayTracingPipelinePropertiesKHR<'static>,

    /// 当前 gpu 的加速结构属性
    pub(crate) _acc_struct_props: vk::PhysicalDeviceAccelerationStructurePropertiesKHR<'static>,

    pub(crate) _mem_props: vk::PhysicalDeviceMemoryProperties,

    pub(crate) gfx_queue_family: GfxQueueFamily,
    pub(crate) compute_queue_family: Option<GfxQueueFamily>,
    pub(crate) transfer_queue_family: Option<GfxQueueFamily>,
}

impl GfxPhysicalDevice {
    /// 创建一个新的物理显卡实例
    ///
    /// 优先选择独立显卡，如果没有则选择第一个可用的显卡
    pub fn new_descrete_physical_device(instance: &ash::Instance) -> Self {
        unsafe {
            instance
                .enumerate_physical_devices()
                .unwrap()
                .iter()
                .map(|pdevice| GfxPhysicalDevice::new(*pdevice, instance))
                // 优先使用独立显卡
                .find_or_first(GfxPhysicalDevice::is_descrete_gpu)
                .unwrap()
        }
    }

    fn new(pdevice: vk::PhysicalDevice, instance: &ash::Instance) -> Self {
        unsafe {
            // 找到符合 ray tracing 条件的 gpu
            let rt_props;
            let basic_props;
            let acc_props;
            {
                let mut pdevice_raytracing_props = vk::PhysicalDeviceRayTracingPipelinePropertiesKHR::default();
                let mut pdevice_acc_props = vk::PhysicalDeviceAccelerationStructurePropertiesKHR::default();
                let mut pdevice_props2 = vk::PhysicalDeviceProperties2::default()
                    .push_next(&mut pdevice_raytracing_props)
                    .push_next(&mut pdevice_acc_props);
                instance.get_physical_device_properties2(pdevice, &mut pdevice_props2);

                // 基础的 props
                basic_props = pdevice_props2.properties;
                let physical_device_name = CStr::from_ptr(basic_props.device_name.as_ptr());
                log::info!("found gpu: {:?}", physical_device_name);

                // ray tracing 属性
                pdevice_raytracing_props.p_next = null_mut();
                rt_props = pdevice_raytracing_props;
                log::debug!("physical deviceray tracing props:\n{:#?}", rt_props);

                // 加速结构 props
                pdevice_acc_props.p_next = null_mut();
                acc_props = pdevice_acc_props;
                log::debug!("physical deivce acceleration structure props:\n{:#?}", acc_props);
            }

            // 找到当前 gpu 支持的 extensions，并打印出来
            let device_extensions = instance.enumerate_device_extension_properties(pdevice).unwrap();
            let device_extension_strs = device_extensions
                .iter()
                .map(|ext| {
                    let ext_name = CStr::from_ptr(ext.extension_name.as_ptr());
                    ext_name.to_str().unwrap().to_string()
                })
                .join("\n");
            log::trace!("physical device supports extensions: {}", device_extension_strs);

            // 找到所有的队列信息并打印出来

            let props_cnt = instance.get_physical_device_queue_family_properties2_len(pdevice);
            let mut queue_familiy_props = vec![vk::QueueFamilyProperties2::default(); props_cnt];
            instance.get_physical_device_queue_family_properties2(pdevice, &mut queue_familiy_props);
            log::debug!("physical device: queue family props:\n{:#?}", queue_familiy_props);

            // 找到符合条的 queue family
            let find_queue_family = |name: String, include_flags: vk::QueueFlags, exclude_flags: vk::QueueFlags| {
                queue_familiy_props
                    .iter()
                    .enumerate()
                    .find(|(_, props)| {
                        !(props.queue_family_properties.queue_flags & include_flags).is_empty()
                            && (props.queue_family_properties.queue_flags & exclude_flags).is_empty()
                    })
                    .map(|(family_idx, props)| GfxQueueFamily {
                        name,
                        queue_family_index: family_idx as u32,
                        queue_flags: props.queue_family_properties.queue_flags,
                        queue_count: props.queue_family_properties.queue_count,
                    })
            };

            // 全能的 Queue：graphics, compute, transfer
            let gfx_queue_family = find_queue_family(
                "gfx".to_string(),
                vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE | vk::QueueFlags::TRANSFER,
                vk::QueueFlags::empty(),
            )
            .unwrap();

            // 仅 Compute
            let compute_queue_family = find_queue_family(
                "compute-only".to_string(),
                vk::QueueFlags::COMPUTE | vk::QueueFlags::TRANSFER,
                vk::QueueFlags::GRAPHICS,
            );

            // 仅 Transfer
            let transfer_queue_family = find_queue_family(
                "transfer-only".to_string(),
                vk::QueueFlags::TRANSFER,
                vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE,
            );

            Self {
                _mem_props: instance.get_physical_device_memory_properties(pdevice),
                _features: instance.get_physical_device_features(pdevice),
                vk_handle: pdevice,
                basic_props,
                rt_pipeline_props: rt_props,
                _acc_struct_props: acc_props,
                gfx_queue_family,
                compute_queue_family,
                transfer_queue_family,
                _device_extensions: device_extensions,
            }
        }
    }

    pub fn destroy(self) {
        // 无需销毁
    }

    #[inline]
    /// 当前 gpu 是否是独立显卡
    pub fn is_descrete_gpu(&self) -> bool {
        self.basic_props.device_type == vk::PhysicalDeviceType::DISCRETE_GPU
    }
}

impl DebugType for GfxPhysicalDevice {
    fn debug_type_name() -> &'static str {
        "GfxPhysicalDevice"
    }

    fn vk_handle(&self) -> impl vk::Handle {
        self.vk_handle
    }
}
