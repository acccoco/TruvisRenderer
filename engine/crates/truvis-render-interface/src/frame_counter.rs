use crate::pipeline_settings::FrameLabel;

/// 语义化的帧标记，表示是对全局 frame_id 的一个包装，提供更清晰的语义
#[derive(Copy, Clone)]
pub struct FrameToken(u64);

impl FrameToken {
    pub fn frame_id(&self) -> u64 {
        self.0
    }
}

pub struct FrameCounter {
    /// 当前的帧序号，一直累加
    frame_id: u64,
    frame_limit: f32,
}

// new & init
impl FrameCounter {
    pub fn new(init_frame_id: u64, frame_limit: f32) -> Self {
        Self {
            frame_id: init_frame_id,
            frame_limit,
        }
    }
}

// update
impl FrameCounter {
    #[inline]
    pub fn next_frame(&mut self) {
        self.frame_id = self.frame_id.wrapping_add(1);
    }
}

// getters
impl FrameCounter {
    const FIF_COUNT: usize = 3;
    #[inline]
    pub fn frame_id(&self) -> u64 {
        self.frame_id
    }
    #[inline]
    pub fn frame_limit(&self) -> f32 {
        self.frame_limit
    }
    #[inline]
    pub fn frame_delta_time_limit_us(&self) -> f32 {
        1000.0 * 1000.0 / self.frame_limit()
    }
    #[inline]
    pub const fn fif_count() -> usize {
        Self::FIF_COUNT
    }
    #[inline]
    pub const fn frame_labes() -> [FrameLabel; Self::FIF_COUNT] {
        [FrameLabel::A, FrameLabel::B, FrameLabel::C]
    }
    #[inline]
    pub fn frame_label(&self) -> FrameLabel {
        FrameLabel::from_usize(self.frame_id as usize % Self::fif_count())
    }
    #[inline]
    pub fn frame_name(&self) -> String {
        format!("[F{}{}]", self.frame_id, self.frame_label())
    }
    #[inline]
    pub fn frame_token(&self) -> FrameToken {
        FrameToken(self.frame_id)
    }
}
