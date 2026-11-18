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

    /// 只在存活分配期间有效，显式 destroy 后清空。
    map_ptr: Option<*mut u8>,
    /// safe POD 原地修改只允许初始化过的映射；只写 staging/SBT 不开放此借用。
    zero_initialized: bool,
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
    /// - mem_map 时映射内存清零，之后的 POD 借用（如 `mapped_slice_ref`）不会读到未定义字节
    pub fn new(
        ctx: GfxResourceCtx<'_>,
        buffer_size: vk::DeviceSize,
        buffer_usage: vk::BufferUsageFlags,
        align: Option<vk::DeviceSize>,
        mem_map: bool,
        name: impl AsRef<str>,
    ) -> Self {
        Self::new_with_mapping(ctx, buffer_size, buffer_usage, align, mem_map, mem_map, name)
    }

    /// 只写映射由调用方先写满实际消费区域；typed slice 与 readback 封装必须选择清零。
    pub(crate) fn new_with_mapping(
        ctx: GfxResourceCtx<'_>, buffer_size: vk::DeviceSize, buffer_usage: vk::BufferUsageFlags,
        align: Option<vk::DeviceSize>, mem_map: bool, zero_mapped: bool, name: impl AsRef<str>,
    ) -> Self {
        assert!(!zero_mapped || mem_map, "zero initialization requires host mapping");
        assert!(buffer_size > 0, "GfxBuffer size must be nonzero");
        if mem_map {
            assert!(buffer_size <= isize::MAX as vk::DeviceSize, "mapped buffer exceeds host addressable range");
        }
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
        let debug_name = name.as_ref().to_owned();
        let allocation_name = format!("Buffer::{debug_name}");
        let (handle, alloc) = with_vma_debug_name(&alloc_ci, &allocation_name, |alloc_ci| unsafe {
            ctx.allocator().create_buffer_with_alignment(&buffer_ci, alloc_ci, align).unwrap()
        });
        let mut buffer = Self {
            handle,
            allocation: Some(alloc),
            size: buffer_size,
            map_ptr: None,
            zero_initialized: zero_mapped,
            device_addr: None,
            debug_name,
            _usage: buffer_usage,
        };
        if mem_map {
            unsafe {
                let ptr = ctx.allocator().map_memory(buffer.allocation.as_mut().unwrap()).unwrap();
                buffer.map_ptr = Some(ptr);
                if zero_mapped {
                    ptr::write_bytes(ptr, 0, buffer_size as usize);
                }
            }
        }
        if buffer_usage.contains(vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS) {
            buffer.device_addr = Some(unsafe {
                ctx.device().get_buffer_device_address(&vk::BufferDeviceAddressInfo::default().buffer(handle))
            });
        }
        ctx.device().set_object_debug_name(handle, allocation_name);
        buffer
    }

    #[inline]
    pub fn new_stage_buffer(ctx: GfxResourceCtx<'_>, size: vk::DeviceSize, debug_name: impl AsRef<str>) -> Self {
        Self::new_with_mapping(ctx, size, vk::BufferUsageFlags::TRANSFER_SRC, None, true, false, debug_name)
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

        let mut allocation = self.allocation.take().expect("live GfxBuffer must own its VMA allocation");
        unsafe {
            if self.map_ptr.take().is_some() {
                ctx.allocator().unmap_memory(&mut allocation);
            }

            ctx.allocator().destroy_buffer(self.handle, &mut allocation);
        }
        self.handle = vk::Buffer::null();
        self.device_addr = None;
        self.size = 0;
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

    /// 写入并 flush 已初始化、无隐式 padding 的宿主数据；owner 必须先等待 GPU 停止访问。
    pub fn transfer_data_by_mmap<T: bytemuck::NoUninit>(&mut self, ctx: GfxResourceCtx<'_>, data: &[T]) {
        let bytes = bytemuck::cast_slice::<T, u8>(data);
        let dst = self.mapped_range_ptr::<u8>(bytes.len());
        if bytes.is_empty() {
            return;
        }
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());

            let allocation = self.allocation.as_ref().expect("GfxBuffer allocation missing");
            ctx.allocator().flush_allocation(allocation, 0, bytes.len() as vk::DeviceSize).unwrap();
        }
    }

    /// 原地修改一个 POD 值；独占借用只约束 CPU，GPU 完成与 readback invalidate 由 owner 保证。
    pub fn transfer<T: bytemuck::Pod>(&mut self, ctx: GfxResourceCtx<'_>, trans_func: &dyn Fn(&mut T)) {
        assert!(self.zero_initialized, "safe POD borrow requires initialized mapping");
        let ptr = self.mapped_range_ptr::<T>(1);
        unsafe {
            trans_func(&mut *ptr);
        }
        let allocation = self.allocation.as_ref().expect("GfxBuffer allocation missing");
        ctx.allocator().flush_allocation(allocation, 0, size_of::<T>() as vk::DeviceSize).unwrap();
    }

    /// 只投影当前分配内的非零大小 POD 范围；调用方的借用决定最终引用是否可变。
    pub(crate) fn mapped_range_ptr<T: bytemuck::Pod>(&self, len: usize) -> *mut T {
        assert!(size_of::<T>() > 0, "mapped element must have nonzero size");
        let bytes = len.checked_mul(size_of::<T>()).expect("mapped range size overflow");
        assert!(bytes <= isize::MAX as usize, "mapped range exceeds host addressable range");
        assert!(bytes as vk::DeviceSize <= self.size, "mapped range exceeds buffer capacity");
        let ptr = self.mapped_ptr().cast::<T>();
        assert!(!ptr.is_null() && ptr.is_aligned(), "mapped range is null or misaligned");
        ptr
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
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        data: &[impl bytemuck::NoUninit],
    ) {
        self.transfer_data_sync2(resource_ctx, immediate_ctx, size_of_val(data) as vk::DeviceSize, |stage| {
            // NoUninit 保证源字节均已初始化；容量由 staging 创建和上传预检共同约束。
            unsafe {
                ptr::copy_nonoverlapping(data.as_ptr().cast::<u8>(), stage.mapped_ptr(), size_of_val(data));
            }
        })
    }

    /// 同步 staging 上传；回调只填写映射内存，不提交或保留 staging 指针。
    ///
    /// sync 表示这个函数是同步等待的，会阻塞运行。
    pub fn transfer_data_sync2(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        total_size: vk::DeviceSize,
        do_with_stage_buffer: impl FnOnce(&GfxBuffer),
    ) {
        assert!(total_size <= self.size, "upload exceeds buffer capacity");
        if total_size == 0 {
            return;
        }
        let stage = Self::new_stage_buffer(resource_ctx, total_size, format!("{}-stage-buffer", self.debug_name));
        do_with_stage_buffer(&stage);
        // 非 coherent 内存必须在提交前可见，类型化上传也复用此处唯一的 flush。
        stage.flush(resource_ctx, 0, total_size);

        immediate_ctx.one_time_exec(
            |cmd| {
                cmd.cmd_copy_buffer(
                    &stage,
                    self,
                    &[vk::BufferCopy {
                        size: total_size,
                        ..Default::default()
                    }],
                )
            },
            format!("{}-transfer-data", self.debug_name),
        );
        stage.destroy(resource_ctx, DestroyReason::ScopeDrop);
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
