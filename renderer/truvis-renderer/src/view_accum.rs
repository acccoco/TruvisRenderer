use truvis_render_foundation::render_view::RenderViewAccumSignature;

/// Truvis 主视图相机签名未变的连续 update 计数，仅供 UI 诊断。
///
/// 首次、相机变化或显式 reset 后从零开始；不表示 GPU 提交、Offline 样本或 temporal 历史有效。
/// 没有 acquired image 时也可能 update，Runtime invalidation 只触发显式 reset。
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
