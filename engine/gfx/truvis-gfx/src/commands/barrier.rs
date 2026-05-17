use ash::vk;

/// barrier 使用的 src 和 dst 访问 mask
#[derive(Copy, Clone)]
pub struct GfxBarrierMask {
    pub src_stage: vk::PipelineStageFlags2,
    pub dst_stage: vk::PipelineStageFlags2,
    pub src_access: vk::AccessFlags2,
    pub dst_access: vk::AccessFlags2,
}

/// 便捷创建 image memory barrier 的结构体
pub struct GfxImageBarrier {
    inner: vk::ImageMemoryBarrier2<'static>,
}

impl Default for GfxImageBarrier {
    fn default() -> Self {
        Self {
            inner: vk::ImageMemoryBarrier2 {
                old_layout: vk::ImageLayout::UNDEFINED,
                new_layout: vk::ImageLayout::UNDEFINED,
                src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                subresource_range: vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::empty(),
                    base_array_layer: 0,
                    layer_count: 1,
                    base_mip_level: 0,
                    level_count: 1,
                },
                ..Default::default()
            },
        }
    }
}

impl GfxImageBarrier {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn inner(&self) -> &vk::ImageMemoryBarrier2<'_> {
        &self.inner
    }

    /// 构建器
    #[inline]
    pub fn queue_family_transfer(mut self, src_queue_family_index: u32, dst_queue_family_index: u32) -> Self {
        self.inner.src_queue_family_index = src_queue_family_index;
        self.inner.dst_queue_family_index = dst_queue_family_index;
        self
    }

    /// 构建器
    #[inline]
    pub fn layout_transfer(mut self, old_layout: vk::ImageLayout, new_layout: vk::ImageLayout) -> Self {
        self.inner.old_layout = old_layout;
        self.inner.new_layout = new_layout;
        self
    }

    /// 构建器
    #[inline]
    pub fn src_mask(mut self, src_stage_mask: vk::PipelineStageFlags2, src_access_mask: vk::AccessFlags2) -> Self {
        self.inner.src_stage_mask = src_stage_mask;
        self.inner.src_access_mask = src_access_mask;
        self
    }

    /// 构建器
    #[inline]
    pub fn dst_mask(mut self, dst_stage_mask: vk::PipelineStageFlags2, dst_access_mask: vk::AccessFlags2) -> Self {
        self.inner.dst_stage_mask = dst_stage_mask;
        self.inner.dst_access_mask = dst_access_mask;
        self
    }

    /// 构建器
    /// layer 和 miplevel 都使用默认值
    #[inline]
    pub fn image_aspect_flag(mut self, aspect_mask: vk::ImageAspectFlags) -> Self {
        self.inner.subresource_range.aspect_mask = aspect_mask;
        self
    }

    /// 构建器
    #[inline]
    pub fn image(mut self, image: vk::Image) -> Self {
        self.inner.image = image;
        self
    }
}

pub struct GfxBufferBarrier {
    inner: vk::BufferMemoryBarrier2<'static>,
}

impl Default for GfxBufferBarrier {
    fn default() -> Self {
        Self {
            inner: vk::BufferMemoryBarrier2 {
                src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                ..Default::default()
            },
        }
    }
}

impl GfxBufferBarrier {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn inner(&self) -> &vk::BufferMemoryBarrier2<'_> {
        &self.inner
    }

    #[inline]
    pub fn src_mask(mut self, src_stage_mask: vk::PipelineStageFlags2, src_access_mask: vk::AccessFlags2) -> Self {
        self.inner.src_stage_mask = src_stage_mask;
        self.inner.src_access_mask = src_access_mask;
        self
    }

    #[inline]
    pub fn dst_mask(mut self, dst_stage_mask: vk::PipelineStageFlags2, dst_access_mask: vk::AccessFlags2) -> Self {
        self.inner.dst_stage_mask = dst_stage_mask;
        self.inner.dst_access_mask = dst_access_mask;
        self
    }

    #[inline]
    pub fn mask(mut self, mask: GfxBarrierMask) -> Self {
        self.inner.src_stage_mask = mask.src_stage;
        self.inner.dst_stage_mask = mask.dst_stage;
        self.inner.src_access_mask = mask.src_access;
        self.inner.dst_access_mask = mask.dst_access;
        self
    }

    #[inline]
    pub fn buffer(mut self, buffer: vk::Buffer, offset: vk::DeviceSize, size: vk::DeviceSize) -> Self {
        self.inner.buffer = buffer;
        self.inner.offset = offset;
        self.inner.size = size;
        self
    }
}
