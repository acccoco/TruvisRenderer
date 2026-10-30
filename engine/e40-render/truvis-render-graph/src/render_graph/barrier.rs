//! Barrier 自动计算
//!
//! 根据资源状态转换自动生成 ImageMemoryBarrier。

use ash::vk;
use truvis_gfx::commands::barrier::GfxImageBarrier;

use crate::render_graph::resource_handle::RgImageHandle;
use crate::render_graph::resource_state::RgImageState;

/// 图像 Barrier 描述
#[derive(Clone, Debug)]
pub struct RgImageBarrierDesc {
    /// 资源句柄（RenderGraph 内部）
    pub handle: RgImageHandle,
    /// 源状态
    pub src_state: RgImageState,
    /// 目标状态
    pub dst_state: RgImageState,
    /// 图像 aspect（COLOR / DEPTH / STENCIL）
    pub aspect: vk::ImageAspectFlags,
}

impl RgImageBarrierDesc {
    /// 创建新的图像 barrier 描述
    pub fn new(handle: RgImageHandle, src_state: RgImageState, dst_state: RgImageState) -> Self {
        Self {
            handle,
            src_state,
            dst_state,
            aspect: vk::ImageAspectFlags::COLOR,
        }
    }

    /// 设置 aspect
    pub fn with_aspect(mut self, aspect: vk::ImageAspectFlags) -> Self {
        self.aspect = aspect;
        self
    }

    /// 检查是否需要 barrier
    ///
    /// 同 layout 的连续只读访问，仅在来源范围包含目标 stage/access 时可跳过。
    /// 新读取范围需要继续建立依赖链，CPU 合并状态不能代替 GPU 可见性操作。
    pub fn needs_barrier(&self) -> bool {
        // Layout 不同一定需要 barrier
        if self.src_state.layout != self.dst_state.layout {
            return true;
        }

        // 有写操作需要 barrier（确保可见性）
        if self.src_state.is_write() || self.dst_state.is_write() {
            return true;
        }

        !self.src_state.stage.contains(self.dst_state.stage) || !self.src_state.access.contains(self.dst_state.access)
    }

    /// 转换为 GfxImageBarrier
    ///
    /// 需要提供实际的 vk::Image handle
    pub fn to_gfx_barrier(&self, image: vk::Image) -> GfxImageBarrier {
        GfxImageBarrier::new()
            .image(image)
            .layout_transfer(self.src_state.layout, self.dst_state.layout)
            .src_mask(self.src_state.stage, self.src_state.src_access())
            .dst_mask(self.dst_state.stage, self.dst_state.access)
            .image_aspect_flag(self.aspect)
            .whole_image()
    }
}

/// Pass 执行前需要的 Barrier 集合
#[derive(Clone, Debug, Default)]
pub struct PassBarriers {
    /// 图像 barriers
    pub image_barriers: Vec<RgImageBarrierDesc>,
}

impl PassBarriers {
    /// 创建空的 barrier 集合
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加图像 barrier
    pub fn add_image_barrier(&mut self, barrier: RgImageBarrierDesc) {
        if barrier.needs_barrier() {
            self.image_barriers.push(barrier);
        }
    }

    /// 检查是否有 barrier
    #[inline]
    pub fn has_barriers(&self) -> bool {
        !self.image_barriers.is_empty()
    }

    /// 获取图像 barrier 数量
    #[inline]
    pub fn image_barrier_count(&self) -> usize {
        self.image_barriers.len()
    }
}
