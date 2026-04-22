//! Barrier 自动计算
//!
//! 根据资源状态转换自动生成 ImageMemoryBarrier 和 BufferMemoryBarrier。

use ash::vk;
use truvis_gfx::commands::barrier::{GfxBufferBarrier, GfxImageBarrier};

use crate::render_graph::{RgBufferHandle, RgBufferState, RgImageHandle, RgImageState};

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
    /// 如果 layout 相同且 access 兼容，可能不需要 barrier
    pub fn needs_barrier(&self) -> bool {
        // Layout 不同一定需要 barrier
        if self.src_state.layout != self.dst_state.layout {
            return true;
        }

        // 有写操作需要 barrier（确保可见性）
        if self.src_state.is_write() || self.dst_state.is_write() {
            return true;
        }

        // 只读到只读可以跳过 barrier
        false
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
    }
}

/// 缓冲区 Barrier 描述
#[derive(Clone, Debug)]
pub struct BufferBarrierDesc {
    /// 资源句柄
    pub handle: RgBufferHandle,
    /// 源状态
    pub src_state: RgBufferState,
    /// 目标状态
    pub dst_state: RgBufferState,
    /// 缓冲区偏移
    pub offset: vk::DeviceSize,
    /// 缓冲区大小（WHOLE_SIZE 表示整个缓冲区）
    pub size: vk::DeviceSize,
}

impl BufferBarrierDesc {
    /// 创建新的缓冲区 barrier 描述
    pub fn new(handle: RgBufferHandle, src_state: RgBufferState, dst_state: RgBufferState) -> Self {
        Self {
            handle,
            src_state,
            dst_state,
            offset: 0,
            size: vk::WHOLE_SIZE,
        }
    }

    /// 检查是否需要 barrier
    pub fn needs_barrier(&self) -> bool {
        // 有写操作需要 barrier
        self.src_state.is_write() || self.dst_state.is_write()
    }

    /// 转换为 GfxBufferBarrier
    pub fn to_gfx_barrier(&self, buffer: vk::Buffer) -> GfxBufferBarrier {
        GfxBufferBarrier::new()
            .buffer(buffer, self.offset, self.size)
            .src_mask(self.src_state.stage, self.src_state.access)
            .dst_mask(self.dst_state.stage, self.dst_state.access)
    }
}

/// Pass 执行前需要的 Barrier 集合
#[derive(Clone, Debug, Default)]
pub struct PassBarriers {
    /// 图像 barriers
    pub image_barriers: Vec<RgImageBarrierDesc>,
    /// 缓冲区 barriers
    pub buffer_barriers: Vec<BufferBarrierDesc>,
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

    /// 添加缓冲区 barrier
    pub fn add_buffer_barrier(&mut self, barrier: BufferBarrierDesc) {
        if barrier.needs_barrier() {
            self.buffer_barriers.push(barrier);
        }
    }

    /// 检查是否有 barrier
    #[inline]
    pub fn has_barriers(&self) -> bool {
        !self.image_barriers.is_empty() || !self.buffer_barriers.is_empty()
    }

    /// 获取图像 barrier 数量
    #[inline]
    pub fn image_barrier_count(&self) -> usize {
        self.image_barriers.len()
    }

    /// 获取缓冲区 barrier 数量
    #[inline]
    pub fn buffer_barrier_count(&self) -> usize {
        self.buffer_barriers.len()
    }
}

#[cfg(test)]
mod tests {
    use slotmap::SlotMap;

    use super::*;

    fn create_test_image_handle() -> RgImageHandle {
        let mut sm: SlotMap<RgImageHandle, ()> = SlotMap::with_key();
        sm.insert(())
    }

    #[test]
    fn test_image_barrier_layout_change() {
        let handle = create_test_image_handle();
        let barrier =
            RgImageBarrierDesc::new(handle, RgImageState::UNDEFINED_TOP, RgImageState::COLOR_ATTACHMENT_WRITE);

        assert!(barrier.needs_barrier());
    }

    #[test]
    fn test_image_barrier_read_to_read() {
        let handle = create_test_image_handle();
        let barrier =
            RgImageBarrierDesc::new(handle, RgImageState::SHADER_READ_FRAGMENT, RgImageState::SHADER_READ_COMPUTE);

        // 同 layout 的只读到只读可以跳过
        // 但这里 layout 可能不同，取决于实际定义
        // 实际上 SHADER_READ_ONLY_OPTIMAL 相同，所以不需要
        assert!(!barrier.needs_barrier());
    }

    #[test]
    fn test_image_barrier_write_to_read() {
        let handle = create_test_image_handle();
        let barrier =
            RgImageBarrierDesc::new(handle, RgImageState::STORAGE_WRITE_COMPUTE, RgImageState::SHADER_READ_FRAGMENT);

        assert!(barrier.needs_barrier());
    }
}
