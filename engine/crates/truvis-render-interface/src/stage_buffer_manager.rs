use truvis_gfx::resources::buffer::GfxBuffer;

use crate::frame_counter::FrameCounter;

pub struct StageBufferManager {
    buffers: [Vec<GfxBuffer>; FrameCounter::fif_count()],
}

// new & init
impl Default for StageBufferManager {
    fn default() -> Self {
        Self::new()
    }
}

impl StageBufferManager {
    pub fn new() -> Self {
        let buffers = FrameCounter::frame_labes().map(|_| Vec::new());
        Self { buffers }
    }
}
impl Drop for StageBufferManager {
    fn drop(&mut self) {
        log::info!("UploadBufferManager dropped.");
    }
}
// destory
impl StageBufferManager {
    pub fn destroy(self) {}
}
// tools
impl StageBufferManager {
    pub fn alloc_buffer(&mut self, frame_counter: &FrameCounter, size: u64, debug_name: &str) -> &mut GfxBuffer {
        let buffer = GfxBuffer::new_stage_buffer(size, debug_name);
        let frame_idx = *frame_counter.frame_label();
        self.buffers[frame_idx].push(buffer);
        self.buffers[frame_idx].last_mut().unwrap()
    }

    pub fn register_stage_buffer(&mut self, frame_counter: &FrameCounter, stage_buffer: GfxBuffer) {
        let frame_idx = *frame_counter.frame_label();
        self.buffers[frame_idx].push(stage_buffer);
    }

    pub fn clear_fif_buffers(&mut self, frame_counter: &FrameCounter) {
        let frame_idx = *frame_counter.frame_label();

        self.buffers[frame_idx].clear();
    }
}
