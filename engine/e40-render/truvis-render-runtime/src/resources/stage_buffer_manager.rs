use truvis_gfx::gfx::GfxResourceCtx;
use truvis_gfx::resources::buffer::GfxBuffer;
use truvis_gfx::resources::lifecycle::DestroyReason;

use truvis_render_foundation::frame_label::FrameLabel;

pub struct StageBufferManager {
    buffers: [Vec<GfxBuffer>; FrameLabel::COUNT],
}

// 创建与初始化
impl Default for StageBufferManager {
    fn default() -> Self {
        Self::new()
    }
}

impl StageBufferManager {
    pub fn new() -> Self {
        let buffers = FrameLabel::ALL.map(|_| Vec::new());
        Self { buffers }
    }
}
impl Drop for StageBufferManager {
    fn drop(&mut self) {
        log::info!("UploadBufferManager dropped.");
    }
}
// 销毁
impl StageBufferManager {
    /// RAII 持有资源的立即释放别名；已保存的 buffer 通过 `Drop` 释放。
    pub fn destroy(mut self, ctx: GfxResourceCtx<'_>) {
        for buffers in &mut self.buffers {
            for buffer in buffers.drain(..) {
                buffer.destroy(ctx, DestroyReason::Shutdown);
            }
        }
    }
}
// 工具函数
impl StageBufferManager {
    pub fn alloc_buffer(
        &mut self,
        ctx: GfxResourceCtx<'_>,
        frame_label: FrameLabel,
        size: u64,
        debug_name: &str,
    ) -> &mut GfxBuffer {
        let buffer = GfxBuffer::new_stage_buffer(ctx, size, debug_name);
        let frame_idx = *frame_label;
        self.buffers[frame_idx].push(buffer);
        self.buffers[frame_idx].last_mut().unwrap()
    }

    pub fn register_stage_buffer(&mut self, frame_label: FrameLabel, stage_buffer: GfxBuffer) {
        let frame_idx = *frame_label;
        self.buffers[frame_idx].push(stage_buffer);
    }

    /// 清理当前 frame label 持有的 staging buffers。
    ///
    /// 这里的 “current frame” 指当前 `FrameLabel` 槽位已经通过 timeline 等待确认安全复用；
    /// 它不是 render target 意义上的 renderer-owned FIF resources，因此命名避免继续使用
    /// `fif_buffers`，防止和窗口尺寸图像 owner 混淆。
    pub fn clear_current_frame_buffers(&mut self, ctx: GfxResourceCtx<'_>, frame_label: FrameLabel) {
        let frame_idx = *frame_label;

        for buffer in self.buffers[frame_idx].drain(..) {
            buffer.destroy(ctx, DestroyReason::DeferredCleanup);
        }
    }
}
