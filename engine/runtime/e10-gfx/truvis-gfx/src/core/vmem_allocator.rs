use std::{mem::ManuallyDrop, ops::Deref};

use ash::vk;

pub struct VMemAllocator {
    /// vk_mem 的 Drop 会调用 vmaDestroyAllocator；仅允许显式 destroy 在 root 存活期触发它。
    inner: Option<ManuallyDrop<vk_mem::Allocator>>,
}

impl VMemAllocator {
    /// allocator 借用 native Instance/Device 及 loader，必须在这些 root 之前显式销毁。
    /// 移动 Rust 的 ash wrapper 不会移动 native 对象，不需要为 wrapper 引入 Pin。
    pub fn new(instance: &ash::Instance, pdevice: vk::PhysicalDevice, device: &ash::Device) -> Self {
        let mut vma_ci = vk_mem::AllocatorCreateInfo::new(instance, device, pdevice);
        vma_ci.vulkan_api_version = vk::API_VERSION_1_3;
        vma_ci.flags = vk_mem::AllocatorCreateFlags::BUFFER_DEVICE_ADDRESS;

        let vma = unsafe { vk_mem::Allocator::new(vma_ci).unwrap() };

        Self {
            inner: Some(ManuallyDrop::new(vma)),
        }
    }

    /// 显式释放 VMA allocator，必须在所有 VMA allocation 销毁后调用。
    pub fn destroy(mut self) {
        if let Some(mut allocator) = self.inner.take() {
            // 先撤销 owner，再执行唯一的 native 释放；后续 wrapper Drop 只检查遗漏。
            unsafe { ManuallyDrop::drop(&mut allocator) };
        }
    }
}

impl Deref for VMemAllocator {
    type Target = vk_mem::Allocator;
    fn deref(&self) -> &Self::Target {
        self.inner.as_ref().expect("VMemAllocator already destroyed")
    }
}

impl Drop for VMemAllocator {
    fn drop(&mut self) {
        debug_assert!(self.inner.is_none(), "VMemAllocator dropped without explicit destroy");
    }
}
