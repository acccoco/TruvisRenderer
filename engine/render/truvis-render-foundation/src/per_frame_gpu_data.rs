use ash::vk;

use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::resources::special_buffers::structured_buffer::GfxStructuredBuffer;
use truvis_shader_binding::gpu;

use crate::frame_counter::{FrameCounter, FrameLabel};

/// 每个 FIF slot 一份的 `PerFrameData` UBO owner。
///
/// runtime 在 prepare 阶段写入当前 frame label，pass 只读取当前 buffer 的
/// device address 或通过全局 per-frame descriptor set 访问它。
pub struct PerFrameGpuData {
    buffers: [GfxStructuredBuffer<gpu::PerFrameData>; FrameCounter::fif_count()],
}

impl PerFrameGpuData {
    pub fn new(ctx: GfxResourceCtx<'_>) -> Self {
        let buffers = FrameCounter::frame_labes().map(|frame_label| {
            GfxStructuredBuffer::<gpu::PerFrameData>::new_ubo(ctx, 1, format!("per-frame-data-buffer-{frame_label}"))
        });
        Self { buffers }
    }

    pub fn destroy(mut self, ctx: GfxResourceCtx<'_>) {
        for buffer in &mut self.buffers {
            buffer.destroy_mut(ctx, DestroyReason::Shutdown);
        }
    }

    #[inline]
    pub fn buffer(&self, frame_label: FrameLabel) -> &GfxStructuredBuffer<gpu::PerFrameData> {
        &self.buffers[*frame_label]
    }

    #[inline]
    pub fn device_address(&self, frame_label: FrameLabel) -> vk::DeviceAddress {
        self.buffer(frame_label).device_address()
    }

    #[inline]
    pub fn write(&self, frame_label: FrameLabel, cmd: &GfxCommandBuffer, data: gpu::PerFrameData) {
        cmd.cmd_update_buffer(self.buffer(frame_label).vk_buffer(), 0, BytesConvert::bytes_of(&data));
    }
}
