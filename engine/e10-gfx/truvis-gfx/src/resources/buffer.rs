use std::ptr;

use ash::vk;
use ash::vk::Handle;
use vk_mem::Alloc;

use crate::{
    foundation::debug_messenger::DebugType,
    gfx::{GfxImmediateCtx, GfxResourceCtx},
    resources::{lifecycle::DestroyReason, vma_debug::with_vma_debug_name},
};

pub struct GfxBuffer {
    handle: vk::Buffer,
    allocation: Option<vk_mem::Allocation>,

    size: vk::DeviceSize,

    /// 在初始化阶段写死
    map_ptr: Option<*mut u8>,
    /// 只有在 buffer usage 包含 SHADER_DEVICE_ADDRESS 时才有值
    device_addr: Option<vk::DeviceAddress>,

    debug_name: String,

    _usage: vk::BufferUsageFlags,
}
impl DebugType for GfxBuffer {
    fn debug_type_name() -> &'static str {
        "GfxBuffer"
    }

    fn vk_handle(&self) -> impl vk::Handle {
        self.handle
    }
}
impl Drop for GfxBuffer {
    fn drop(&mut self) {
        debug_assert!(
            self.handle.is_null() && self.allocation.is_none(),
            "GfxBuffer '{}' dropped without explicit owner release",
            self.debug_name
        );
    }
}
// 初始化与销毁
impl GfxBuffer {
    /// - align: 当 buffer 处于一个大的 memory block 中时，align 用来指定 buffer 的起始 offset,
    ///   其实地址的内存对齐，默认对齐到 8 字节
    /// - 优先使用 device memory
    pub fn new(
        ctx: GfxResourceCtx<'_>,
        buffer_size: vk::DeviceSize,
        buffer_usage: vk::BufferUsageFlags,
        align: Option<vk::DeviceSize>,
        mem_map: bool,
        name: impl AsRef<str>,
    ) -> Self {
        // 不允许 UNIFORM + DBA 的组合，会有隐患
        if buffer_usage.contains(vk::BufferUsageFlags::UNIFORM_BUFFER)
            && buffer_usage.contains(vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS)
        {
            panic!("GfxBuffer::new: UNIFORM_BUFFER + SHADER_DEVICE_ADDRESS is not allowed!");
        }

        let buffer_ci = vk::BufferCreateInfo::default().size(buffer_size).usage(buffer_usage);
        let alloc_ci = vk_mem::AllocationCreateInfo {
            usage: vk_mem::MemoryUsage::AutoPreferDevice,
            flags: if mem_map {
                vk_mem::AllocationCreateFlags::HOST_ACCESS_RANDOM
            } else {
                vk_mem::AllocationCreateFlags::empty()
            },
            ..Default::default()
        };

        let align = align.unwrap_or(8);
        let allocation_name = format!("Buffer::{}", name.as_ref());
        let (buffer, mut alloc) = with_vma_debug_name(&alloc_ci, &allocation_name, |alloc_ci| unsafe {
            ctx.allocator().create_buffer_with_alignment(&buffer_ci, alloc_ci, align).unwrap()
        });

        let mut mapped_ptr = None;
        if mem_map {
            unsafe {
                let allocator = ctx.allocator();
                mapped_ptr = Some(allocator.map_memory(&mut alloc).unwrap());
            }
        }

        let mut device_addr = None;
        if buffer_usage.contains(vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS) {
            let gfx_device = ctx.device();
            unsafe {
                device_addr =
                    Some(gfx_device.get_buffer_device_address(&vk::BufferDeviceAddressInfo::default().buffer(buffer)));
            }
        }

        ctx.device().set_object_debug_name(buffer, allocation_name);
        Self {
            handle: buffer,
            allocation: Some(alloc),
            size: buffer_size,
            map_ptr: mapped_ptr,
            device_addr,

            debug_name: name.as_ref().to_string(),

            _usage: buffer_usage,
        }
    }

    #[inline]
    pub fn new_stage_buffer(ctx: GfxResourceCtx<'_>, size: vk::DeviceSize, debug_name: impl AsRef<str>) -> Self {
        Self::new(ctx, size, vk::BufferUsageFlags::TRANSFER_SRC, None, true, debug_name)
    }

    #[inline]
    pub fn new_readback_buffer(ctx: GfxResourceCtx<'_>, size: vk::DeviceSize, debug_name: impl AsRef<str>) -> Self {
        Self::new(ctx, size, vk::BufferUsageFlags::TRANSFER_DST, None, true, debug_name)
    }
}
// 销毁
impl GfxBuffer {
    #[inline]
    pub fn destroy(mut self, ctx: GfxResourceCtx<'_>, reason: DestroyReason) {
        self.destroy_mut(ctx, reason);
    }

    pub fn destroy_mut(&mut self, ctx: GfxResourceCtx<'_>, reason: DestroyReason) {
        if self.handle.is_null() {
            return;
        }

        log::trace!("Destroying GfxBuffer name={} raw={:#x} reason={}", self.debug_name, self.handle.as_raw(), reason);

        let Some(mut allocation) = self.allocation.take() else {
            debug_assert!(false, "GfxBuffer '{}' has handle but no VMA allocation", self.debug_name);
            self.handle = vk::Buffer::null();
            return;
        };

        unsafe {
            if self.map_ptr.take().is_some() {
                ctx.allocator().unmap_memory(&mut allocation);
            }

            ctx.allocator().destroy_buffer(self.handle, &mut allocation);
        }
        self.handle = vk::Buffer::null();
    }
}
// 访问器
impl GfxBuffer {
    #[inline]
    pub fn vk_buffer(&self) -> vk::Buffer {
        self.handle
    }

    #[inline]
    pub fn device_address(&self) -> vk::DeviceAddress {
        self.device_addr.expect(
            "Buffer does not have device address, please make sure the buffer usage contains SHADER_DEVICE_ADDRESS",
        )
    }

    #[inline]
    pub fn size(&self) -> vk::DeviceSize {
        self.size
    }

    #[inline]
    pub fn debug_name(&self) -> &str {
        &self.debug_name
    }
}
// 工具函数
impl GfxBuffer {
    #[inline]
    pub fn mapped_ptr(&self) -> *mut u8 {
        self.map_ptr.expect("Buffer is not mapped, please call map() before using mapped_ptr()")
    }

    #[inline]
    pub fn flush(&self, ctx: GfxResourceCtx<'_>, offset: vk::DeviceSize, size: vk::DeviceSize) {
        let allocator = ctx.allocator();
        let allocation = self.allocation.as_ref().expect("GfxBuffer allocation missing");
        allocator.flush_allocation(allocation, offset, size).unwrap();
    }

    #[inline]
    pub fn invalidate(&self, ctx: GfxResourceCtx<'_>, offset: vk::DeviceSize, size: vk::DeviceSize) {
        let allocator = ctx.allocator();
        let allocation = self.allocation.as_ref().expect("GfxBuffer allocation missing");
        allocator.invalidate_allocation(allocation, offset, size).unwrap();
    }

    /// 通过 mem map 的方式将 data 传入到 buffer 中
    pub fn transfer_data_by_mmap<T>(&self, ctx: GfxResourceCtx<'_>, data: &[T])
    where
        T: Sized + Copy,
    {
        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr() as *const u8, self.mapped_ptr(), size_of_val(data));

            let allocation = self.allocation.as_ref().expect("GfxBuffer allocation missing");
            ctx.allocator().flush_allocation(allocation, 0, size_of_val(data) as vk::DeviceSize).unwrap();
        }
    }

    // BUG 可能需要考虑内存对齐
    pub fn transfer<T: bytemuck::Pod>(&self, ctx: GfxResourceCtx<'_>, trans_func: &dyn Fn(&mut T)) {
        unsafe {
            let ptr = self.map_ptr.unwrap() as *mut T;

            trans_func(&mut *ptr);
        }
        let allocation = self.allocation.as_ref().expect("GfxBuffer allocation missing");
        ctx.allocator().flush_allocation(allocation, 0, size_of::<T>() as vk::DeviceSize).unwrap();
    }

    /// 创建一个临时的 stage buffer，先将数据放入 stage buffer，再 transfer 到
    /// 自身
    ///
    /// sync 表示这个函数是同步等待的，会阻塞运行
    ///
    /// # 说明
    /// * 避免使用这个将 *小块* 数据从内存传到 GPU，推荐使用 cmd transfer
    /// * 这个应该是用来传输大块数据的
    pub fn transfer_data_sync(
        &self,
        resource_ctx: GfxResourceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        data: &[impl Sized + Copy],
    ) {
        let stage_buffer = Self::new_stage_buffer(
            resource_ctx,
            size_of_val(data) as vk::DeviceSize,
            format!("{}-stage-buffer", self.debug_name),
        );

        stage_buffer.transfer_data_by_mmap(resource_ctx, data);

        let cmd_name = format!("{}-transfer-data", &self.debug_name);
        immediate_ctx.one_time_exec(
            |cmd| {
                cmd.cmd_copy_buffer(
                    &stage_buffer,
                    self,
                    &[vk::BufferCopy {
                        size: size_of_val(data) as vk::DeviceSize,
                        ..Default::default()
                    }],
                );
            },
            &cmd_name,
        );
        stage_buffer.destroy(resource_ctx, DestroyReason::ScopeDrop);
    }

    /// 创建一个临时的 stage buffer，先将数据放入 stage buffer，再 transfer 到
    /// 自身
    ///
    /// sync 表示这个函数是同步等待的，会阻塞运行
    ///
    /// # 说明
    /// * 避免使用这个将 *小块* 数据从内存传到 GPU，推荐使用 cmd transfer
    /// * 这个应该是用来传输大块数据的
    pub fn transfer_data_sync2(
        &self,
        resource_ctx: GfxResourceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        total_size: vk::DeviceSize,
        do_with_stage_buffer: impl FnOnce(&GfxBuffer),
    ) {
        let stage_buffer =
            Self::new_stage_buffer(resource_ctx, total_size, format!("{}-stage-buffer", self.debug_name));

        do_with_stage_buffer(&stage_buffer);

        let cmd_name = format!("{}-transfer-data", &self.debug_name);
        immediate_ctx.one_time_exec(
            |cmd| {
                cmd.cmd_copy_buffer(
                    &stage_buffer,
                    self,
                    &[vk::BufferCopy {
                        size: total_size,
                        ..Default::default()
                    }],
                );
            },
            &cmd_name,
        );
        stage_buffer.destroy(resource_ctx, DestroyReason::ScopeDrop);
    }

    /// 清空 buffer 内容为 0
    pub fn clear(&mut self, immediate_ctx: GfxImmediateCtx<'_>) {
        immediate_ctx.one_time_exec(
            |cmd| {
                cmd.cmd_fill_buffer(self.vk_buffer(), 0, vk::WHOLE_SIZE, 0);
            },
            "clear-buffer",
        );
    }
}
