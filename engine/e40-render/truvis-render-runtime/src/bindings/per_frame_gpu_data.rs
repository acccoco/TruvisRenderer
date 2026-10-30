use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_shader_binding::gpu;

use truvis_render_foundation::frame_label::FrameLabel;

/// 每个 FIF slot 一份的 `PerFrameData` UBO owner。
///
/// runtime 在 prepare 阶段写入当前 frame label，pass 通过全局 per-frame descriptor set 访问它。
pub struct PerFrameGpuData {
    buffers: [GfxStructuredBuffer<gpu::engine::frame::PerFrameData>; FrameLabel::COUNT],
}

impl PerFrameGpuData {
    pub fn new(ctx: GfxResourceCtx<'_>) -> Self {
        Self {
            buffers: FrameLabel::ALL
                .map(|label| GfxStructuredBuffer::new_ubo(ctx, 1, format!("per-frame-data-buffer-{label}"))),
        }
    }

    /// 调用方确认全部 FIF 不再被 GPU 引用后显式销毁。
    pub fn destroy(mut self, ctx: GfxResourceCtx<'_>) {
        for buffer in &mut self.buffers {
            buffer.destroy_mut(ctx, DestroyReason::Shutdown);
        }
    }

    #[inline]
    pub fn buffer(&self, frame_label: FrameLabel) -> &GfxStructuredBuffer<gpu::engine::frame::PerFrameData> {
        &self.buffers[*frame_label]
    }

    #[inline]
    /// 仅录制更新；调用方保证 FIF 已可复用，并在 shader 消费前建立 transfer-to-uniform 屏障。
    pub fn write(&self, frame_label: FrameLabel, cmd: &GfxCommandBuffer, data: gpu::engine::frame::PerFrameData) {
        cmd.cmd_update_buffer(self.buffer(frame_label).vk_buffer(), 0, BytesConvert::bytes_of(&data));
    }
}
