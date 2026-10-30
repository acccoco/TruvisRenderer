use std::fmt;

use ash::vk;

use crate::ffi;

/// 必需的设备命令未由当前 ash loader 链路提供。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadError {
    pub command: &'static str,
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Vulkan command unavailable: {}", self.command)
    }
}

impl std::error::Error for LoadError {}

/// 按值持有的设备函数表，与 ash extension loader 一样不拥有 Vulkan 对象。
/// GfxDevice 持有本 wrapper；RenderThread 上最后一次命令调用必须早于 Device / Instance / Entry 的销毁。
pub struct Device {
    dispatch: ffi::TruvixxVkDeviceDispatch,
}

impl Device {
    /// 使用已有 ash Instance 的原生查询入口，不另外加载系统 loader 或 Streamline。
    /// 本方法不保存 ash 引用；Rust wrapper 存活不等于原生对象或 loader 仍有效。
    ///
    /// # Safety
    /// device 必须是由 instance 创建且尚未销毁的原生设备；instance 的函数表及所属 loader
    /// 必须在查询期间有效，不得与销毁或卸载并发。后续命令调用仍需保持同一链路存活。
    pub unsafe fn new(instance: &ash::Instance, device: &ash::Device) -> Result<Self, LoadError> {
        let mut dispatch = ffi::TruvixxVkDeviceDispatch::default();
        let loaded = unsafe {
            ffi::truvixx_vk_device_init(
                device.handle(),
                Some(instance.fp_v1_0().get_device_proc_addr),
                &mut dispatch,
            )
        };
        if loaded == vk::FALSE {
            return Err(LoadError {
                command: "vkCmdPushDescriptorSetKHR",
            });
        }
        Ok(Self { dispatch })
    }

    /// 使用已有 ash descriptor 数组同步录制，不保留数组或其 pNext / 嵌套指针。
    ///
    /// # Panics
    /// descriptor_writes 为空或长度超出 u32 时，在进入 native 前 panic。
    ///
    /// # Safety
    /// command_buffer、layout 和 descriptor 资源必须属于初始化时的 Device，并满足
    /// vkCmdPushDescriptorSetKHR 的有效用法，包括 push descriptor layout、
    /// 合法录制状态和外部同步。调用期间所有 CPU 指针及 Device / Instance / Entry 必须有效；
    /// 被引用的 GPU 资源必须保持到相关提交完成。本接口在项目中只由 RenderThread 调用。
    #[inline]
    pub unsafe fn cmd_push_descriptor_set(
        &self,
        command_buffer: vk::CommandBuffer,
        pipeline_bind_point: vk::PipelineBindPoint,
        layout: vk::PipelineLayout,
        set: u32,
        descriptor_writes: &[vk::WriteDescriptorSet<'_>],
    ) {
        assert!(!descriptor_writes.is_empty(), "push descriptor writes must not be empty");
        let count = u32::try_from(descriptor_writes.len()).expect("descriptor write count exceeds u32");
        unsafe {
            ffi::truvixx_vk_cmd_push_descriptor_set_khr(
                &self.dispatch,
                command_buffer,
                pipeline_bind_point,
                layout,
                set,
                count,
                descriptor_writes.as_ptr(),
            );
        }
    }
}
