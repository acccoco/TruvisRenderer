use truvis_render_foundation::render_view::RenderViewAccumSignature;

/// Truvis 主视图的连续帧计数。
///
/// 这是 Renderer-owned UI/temporal 状态；Runtime 只提供通用 history invalidation 事件。
#[derive(Copy, Clone, Default)]
pub struct ViewAccumState {
    last_signature: Option<RenderViewAccumSignature>,
    accum_frames_num: usize,
}

impl ViewAccumState {
    pub fn update(&mut self, signature: RenderViewAccumSignature) {
        if self.last_signature == Some(signature) {
            self.accum_frames_num = self.accum_frames_num.saturating_add(1);
        } else {
            self.last_signature = Some(signature);
            self.accum_frames_num = 0;
        }
    }

    pub fn reset(&mut self) {
        self.last_signature = None;
        self.accum_frames_num = 0;
    }

    pub fn accum_frames_num(&self) -> usize {
        self.accum_frames_num
    }
}
