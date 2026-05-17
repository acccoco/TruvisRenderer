//! 资源状态定义
//!
//! 封装 Vulkan 的 pipeline stage、access mask 和 image layout，
//! 提供预定义的常用状态组合。

use ash::vk;

// TODO RgImageState 可以考虑提升到 Gfx 里面去
/// 图像资源状态
///
/// 描述图像在某个 Pass 中的使用方式，用于自动计算 barrier。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RgImageState {
    /// Pipeline stage 标志
    pub stage: vk::PipelineStageFlags2,
    /// Access mask 标志
    pub access: vk::AccessFlags2,
    /// Image layout 状态
    pub layout: vk::ImageLayout,
}

impl Default for RgImageState {
    fn default() -> Self {
        Self::UNDEFINED_TOP
    }
}

// new & 常量定义
impl RgImageState {
    /// 创建自定义状态
    #[inline]
    pub const fn new(stage: vk::PipelineStageFlags2, access: vk::AccessFlags2, layout: vk::ImageLayout) -> Self {
        Self { stage, access, layout }
    }

    pub const UNDEFINED_TOP: Self =
        Self::new(vk::PipelineStageFlags2::TOP_OF_PIPE, vk::AccessFlags2::NONE, vk::ImageLayout::UNDEFINED);

    pub const UNDEFINED_BOTTOM: Self =
        Self::new(vk::PipelineStageFlags2::BOTTOM_OF_PIPE, vk::AccessFlags2::NONE, vk::ImageLayout::UNDEFINED);

    pub const GENERAL: Self = Self::new(
        vk::PipelineStageFlags2::ALL_COMMANDS,
        vk::AccessFlags2::from_raw(vk::AccessFlags2::MEMORY_READ.as_raw() | vk::AccessFlags2::MEMORY_WRITE.as_raw()),
        vk::ImageLayout::GENERAL,
    );

    pub const COLOR_ATTACHMENT_WRITE: Self = Self::new(
        vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
        vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
    );

    pub const COLOR_ATTACHMENT_READ_WRITE: Self = Self::new(
        vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
        vk::AccessFlags2::from_raw(
            vk::AccessFlags2::COLOR_ATTACHMENT_READ.as_raw() | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE.as_raw(),
        ),
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
    );

    pub const DEPTH_ATTACHMENT_WRITE: Self = Self::new(
        vk::PipelineStageFlags2::from_raw(
            vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS.as_raw()
                | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS.as_raw(),
        ),
        vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
        vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
    );

    pub const DEPTH_ATTACHMENT_READ_WRITE: Self = Self::new(
        vk::PipelineStageFlags2::from_raw(
            vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS.as_raw()
                | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS.as_raw(),
        ),
        vk::AccessFlags2::from_raw(
            vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ.as_raw()
                | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE.as_raw(),
        ),
        vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
    );

    pub const SHADER_READ_FRAGMENT: Self = Self::new(
        vk::PipelineStageFlags2::FRAGMENT_SHADER,
        vk::AccessFlags2::SHADER_SAMPLED_READ,
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
    );

    pub const SHADER_READ_COMPUTE: Self = Self::new(
        vk::PipelineStageFlags2::COMPUTE_SHADER,
        vk::AccessFlags2::SHADER_SAMPLED_READ,
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
    );

    pub const SHADER_READ_RAY_TRACING: Self = Self::new(
        vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR,
        vk::AccessFlags2::SHADER_SAMPLED_READ,
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
    );

    pub const STORAGE_READ_COMPUTE: Self = Self::new(
        vk::PipelineStageFlags2::COMPUTE_SHADER,
        vk::AccessFlags2::SHADER_STORAGE_READ,
        vk::ImageLayout::GENERAL,
    );

    pub const STORAGE_WRITE_COMPUTE: Self = Self::new(
        vk::PipelineStageFlags2::COMPUTE_SHADER,
        vk::AccessFlags2::SHADER_STORAGE_WRITE,
        vk::ImageLayout::GENERAL,
    );

    pub const STORAGE_READ_WRITE_COMPUTE: Self = Self::new(
        vk::PipelineStageFlags2::COMPUTE_SHADER,
        vk::AccessFlags2::from_raw(
            vk::AccessFlags2::SHADER_STORAGE_READ.as_raw() | vk::AccessFlags2::SHADER_STORAGE_WRITE.as_raw(),
        ),
        vk::ImageLayout::GENERAL,
    );

    pub const STORAGE_WRITE_RAY_TRACING: Self = Self::new(
        vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR,
        vk::AccessFlags2::SHADER_STORAGE_WRITE,
        vk::ImageLayout::GENERAL,
    );

    pub const STORAGE_READ_WRITE_RAY_TRACING: Self = Self::new(
        vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR,
        vk::AccessFlags2::from_raw(
            vk::AccessFlags2::SHADER_STORAGE_READ.as_raw() | vk::AccessFlags2::SHADER_STORAGE_WRITE.as_raw(),
        ),
        vk::ImageLayout::GENERAL,
    );

    pub const TRANSFER_SRC: Self = Self::new(
        vk::PipelineStageFlags2::TRANSFER,
        vk::AccessFlags2::TRANSFER_READ,
        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
    );

    pub const TRANSFER_DST: Self = Self::new(
        vk::PipelineStageFlags2::TRANSFER,
        vk::AccessFlags2::TRANSFER_WRITE,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
    );

    pub const PRESENT_BOTTOM: Self =
        Self::new(vk::PipelineStageFlags2::BOTTOM_OF_PIPE, vk::AccessFlags2::NONE, vk::ImageLayout::PRESENT_SRC_KHR);
}

// 辅助方法
impl RgImageState {
    /// 写操作的 access flags
    const WRITE_ACCESS: vk::AccessFlags2 = vk::AccessFlags2::from_raw(
        vk::AccessFlags2::SHADER_STORAGE_WRITE.as_raw()
            | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE.as_raw()
            | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE.as_raw()
            | vk::AccessFlags2::TRANSFER_WRITE.as_raw()
            | vk::AccessFlags2::MEMORY_WRITE.as_raw(),
    );

    /// 检查是否为写操作
    #[inline]
    pub fn is_write(&self) -> bool {
        self.access.intersects(Self::WRITE_ACCESS)
    }

    /// 检查是否为只读操作
    #[inline]
    pub fn is_read_only(&self) -> bool {
        !self.is_write()
    }

    /// 获取用于 barrier src 的 access。
    ///
    /// 读后写和 layout transition 也需要保留读访问，确保后续写入等待先前读取完成。
    #[inline]
    pub fn src_access(&self) -> vk::AccessFlags2 {
        self.access
    }
}
