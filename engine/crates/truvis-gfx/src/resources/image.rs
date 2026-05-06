use ash::vk;
use ash::vk::Handle;
use vk_mem::{Alloc, Allocation};

use crate::{
    commands::{barrier::GfxImageBarrier, command_buffer::GfxCommandBuffer},
    foundation::debug_messenger::DebugType,
    gfx::{GfxImmediateCtx, GfxResourceCtx},
    resources::{buffer::GfxBuffer, lifecycle::DestroyReason, vma_debug::with_vma_debug_name},
};

/// Vulkan 格式相关的工具类
pub struct VulkanFormatUtils;
impl VulkanFormatUtils {
    /// 计算指定 Vulkan 格式下每个像素需要的字节数
    ///
    /// # 参数
    /// * `format` - Vulkan 图像格式
    ///
    /// # 返回
    /// 每个像素的字节数
    ///
    /// # Panics
    /// 当遇到不支持的格式时会 panic
    pub fn pixel_size_in_bytes(format: vk::Format) -> usize {
        // 根据 vulkan specification 得到的 format 顺序
        const BYTE_3_FORMAT: [(vk::Format, vk::Format); 1] = [(vk::Format::R8G8B8_UNORM, vk::Format::B8G8R8_SRGB)];
        const BYTE_4_FORMAT: [(vk::Format, vk::Format); 1] = [(vk::Format::R8G8B8A8_UNORM, vk::Format::B8G8R8A8_SRGB)];
        const BYTE_6_FORMAT: [(vk::Format, vk::Format); 1] =
            [(vk::Format::R16G16B16_UNORM, vk::Format::R16G16B16_SFLOAT)];
        const BYTE_8_FORMAT: [(vk::Format, vk::Format); 1] =
            [(vk::Format::R16G16B16A16_UNORM, vk::Format::R16G16B16A16_SFLOAT)];

        let is_in_format_region = |format: vk::Format, regions: &[(vk::Format, vk::Format)]| {
            let n = format.as_raw();
            regions.iter().any(|(begin, end)| begin.as_raw() <= n && n < end.as_raw())
        };

        match format {
            f if is_in_format_region(f, &BYTE_3_FORMAT) => 3,
            f if is_in_format_region(f, &BYTE_4_FORMAT) => 4,
            f if is_in_format_region(f, &BYTE_6_FORMAT) => 6,
            f if is_in_format_region(f, &BYTE_8_FORMAT) => 8,
            _ => panic!("unsupported format: {:?}", format),
        }
    }
}

/// Image 来源枚举
pub enum ImageSource {
    /// 由 VMA 分配的 Image
    Allocated(Allocation),
    /// 外部 Image（例如 Swapchain Image），不管理其内存生命周期
    External,
}

pub struct GfxImage {
    handle: vk::Image,
    source: ImageSource,

    extent: vk::Extent3D,
    format: vk::Format,

    name: String,
}

// 访问器
impl GfxImage {
    #[inline]
    pub fn width(&self) -> u32 {
        self.extent.width
    }

    #[inline]
    pub fn height(&self) -> u32 {
        self.extent.height
    }

    #[inline]
    pub fn handle(&self) -> vk::Image {
        self.handle
    }

    #[inline]
    pub fn format(&self) -> vk::Format {
        self.format
    }

    #[inline]
    pub fn debug_name(&self) -> &str {
        &self.name
    }
}

// 创建与初始化
impl GfxImage {
    pub fn new(
        ctx: GfxResourceCtx<'_>,
        image_info: &GfxImageCreateInfo,
        alloc_info: &vk_mem::AllocationCreateInfo,
        debug_name: &str,
    ) -> Self {
        let allocator = ctx.allocator();
        let gfx_device = ctx.device();
        let (image, alloc) = with_vma_debug_name(alloc_info, debug_name, |alloc_info| unsafe {
            allocator.create_image(&image_info.as_info(), alloc_info).unwrap()
        });
        let image = Self {
            handle: image,
            source: ImageSource::Allocated(alloc),
            extent: image_info.inner.extent,
            format: image_info.inner.format,

            name: debug_name.to_string(),
        };
        gfx_device.set_debug_name(&image, debug_name);
        image
    }

    pub fn from_external(
        ctx: GfxResourceCtx<'_>,
        image: vk::Image,
        extent: vk::Extent3D,
        format: vk::Format,
        name: impl AsRef<str>,
    ) -> Self {
        let gfx_device = ctx.device();
        let image = Self {
            handle: image,
            source: ImageSource::External,
            extent,
            format,

            name: name.as_ref().to_string(),
        };
        gfx_device.set_debug_name(&image, name.as_ref());
        image
    }

    // TODO 考虑将 GfxImage::from_rgba8 放入 UploadManager 中，并提供异步版本
    /// 根据 RGBA8_UNORM 的 data 创建 image
    pub fn from_rgba8(
        resource_ctx: GfxResourceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        width: u32,
        height: u32,
        data: &[u8],
        name: impl AsRef<str>,
    ) -> Self {
        let image_create_info = GfxImageCreateInfo::new_image_2d_info(
            vk::Extent2D { width, height },
            vk::Format::R8G8B8A8_UNORM,
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
        );
        let image = Self::new(
            resource_ctx,
            &image_create_info,
            &vk_mem::AllocationCreateInfo {
                usage: vk_mem::MemoryUsage::AutoPreferDevice,
                ..Default::default()
            },
            name.as_ref(),
        );

        let stage_buffer =
            immediate_ctx.one_time_exec(|cmd| image.transfer_data(resource_ctx, cmd, data), name.as_ref());
        stage_buffer.destroy(resource_ctx, DestroyReason::ScopeDrop);

        image
    }
}
impl DebugType for GfxImage {
    fn debug_type_name() -> &'static str {
        "GfxImage2D"
    }

    fn vk_handle(&self) -> impl vk::Handle {
        self.handle
    }
}

// 销毁
impl GfxImage {
    pub fn destroy(mut self, ctx: GfxResourceCtx<'_>, reason: DestroyReason) {
        self.release(ctx, reason);
    }

    fn release(&mut self, ctx: GfxResourceCtx<'_>, reason: DestroyReason) {
        if self.handle.is_null() {
            return;
        }

        log::debug!("Destroying GfxImage name={} raw={:#x} reason={}", self.name, self.handle.as_raw(), reason);

        match &mut self.source {
            ImageSource::External => (),
            ImageSource::Allocated(allocation) => unsafe { ctx.allocator().destroy_image(self.handle, allocation) },
        }
        self.handle = vk::Image::null();
    }
}
impl Drop for GfxImage {
    fn drop(&mut self) {
        debug_assert!(
            self.handle.is_null(),
            "GfxImage '{}' dropped without explicit manager/lifecycle-owner release",
            self.name
        );
    }
}

// 工具函数
impl GfxImage {
    /// # 实现步骤
    /// 1. 创建一个 staging buffer，用于存放待复制的数据
    /// 2. 将数据复制到 staging buffer
    /// 3. 进行图像布局转换
    /// 4. 将 staging buffer 的数据复制到图像
    /// 5. 进行图像布局转换
    pub fn transfer_data(
        &self,
        resource_ctx: GfxResourceCtx<'_>,
        command_buffer: &GfxCommandBuffer,
        data: &[u8],
    ) -> GfxBuffer {
        let pixels_cnt = self.width() * self.height();
        assert_eq!(data.len(), VulkanFormatUtils::pixel_size_in_bytes(self.format()) * pixels_cnt as usize);

        let stage_buffer =
            GfxBuffer::new_stage_buffer(resource_ctx, size_of_val(data) as vk::DeviceSize, "image-stage-buffer");
        stage_buffer.transfer_data_by_mmap(resource_ctx, data);

        // 1. 转换 image layout
        // 2. 将 buffer 复制到 image
        // 3. 再次转换 layout，让 fragment shader 可读
        {
            let image_barrier = GfxImageBarrier::new()
                .image(self.handle)
                .src_mask(vk::PipelineStageFlags2::TOP_OF_PIPE, vk::AccessFlags2::empty())
                .dst_mask(vk::PipelineStageFlags2::TRANSFER, vk::AccessFlags2::TRANSFER_WRITE)
                .layout_transfer(vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .image_aspect_flag(vk::ImageAspectFlags::COLOR);
            command_buffer.image_memory_barrier(vk::DependencyFlags::empty(), std::slice::from_ref(&image_barrier));

            let buffer_image_copy = vk::BufferImageCopy2::default()
                .buffer_offset(0)
                .buffer_row_length(0)
                .buffer_image_height(0)
                .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
                .image_extent(vk::Extent3D {
                    width: self.width(),
                    height: self.height(),
                    depth: 1,
                })
                .image_subresource(vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                });
            command_buffer.cmd_copy_buffer_to_image(
                &vk::CopyBufferToImageInfo2::default()
                    .src_buffer(stage_buffer.vk_buffer())
                    .dst_image(self.handle)
                    .dst_image_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .regions(std::slice::from_ref(&buffer_image_copy)),
            );

            let image_barrier = GfxImageBarrier::new()
                .image(self.handle)
                .src_mask(vk::PipelineStageFlags2::TRANSFER, vk::AccessFlags2::TRANSFER_WRITE)
                .dst_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER, vk::AccessFlags2::SHADER_READ)
                .layout_transfer(vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .image_aspect_flag(vk::ImageAspectFlags::COLOR);
            command_buffer.image_memory_barrier(vk::DependencyFlags::empty(), std::slice::from_ref(&image_barrier));
        }

        stage_buffer
    }
}

pub struct GfxImageCreateInfo {
    inner: vk::ImageCreateInfo<'static>,

    queue_family_indices: Vec<u32>,
}
impl GfxImageCreateInfo {
    #[inline]
    pub fn new_image_2d_info(extent: vk::Extent2D, format: vk::Format, usage: vk::ImageUsageFlags) -> Self {
        Self {
            inner: vk::ImageCreateInfo {
                image_type: vk::ImageType::TYPE_2D,
                format,
                extent: extent.into(),
                mip_levels: 1,
                array_layers: 1,
                samples: vk::SampleCountFlags::TYPE_1,
                tiling: vk::ImageTiling::OPTIMAL,
                usage,
                sharing_mode: vk::SharingMode::EXCLUSIVE,
                // spec 上面说，这里只能是 UNDEFINED 或者 PREINITIALIZED
                initial_layout: vk::ImageLayout::UNDEFINED,
                ..Default::default()
            },
            queue_family_indices: Vec::new(),
        }
    }

    #[inline]
    pub fn as_info(&self) -> vk::ImageCreateInfo<'_> {
        self.inner.queue_family_indices(&self.queue_family_indices)
    }

    // 构建器
    #[inline]
    pub fn queue_family_indices(mut self, queue_family_indices: &[u32]) -> Self {
        self.inner.sharing_mode = vk::SharingMode::CONCURRENT;
        self.queue_family_indices = queue_family_indices.into();

        self.inner.queue_family_index_count = self.queue_family_indices.len() as u32;
        self.inner.p_queue_family_indices = self.queue_family_indices.as_ptr();
        self
    }
}
