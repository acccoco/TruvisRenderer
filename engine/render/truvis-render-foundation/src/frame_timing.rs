use crate::frame_counter::{FrameCounter, FrameLabel, FrameToken};

/// 渲染帧序号、FIF label 与帧时间快照。
///
/// `FrameTimer` 负责从 wall clock 采样；本类型只保存当前帧对 runtime、app 和 pass
/// 可见的稳定快照，并提供 frame token 给延迟回收系统使用。
pub struct FrameTiming {
    frame_counter: FrameCounter,
    delta_time_s: f32,
    total_time_s: f32,
}

impl FrameTiming {
    pub fn new(frame_counter: FrameCounter) -> Self {
        Self {
            frame_counter,
            delta_time_s: 0.0,
            total_time_s: 0.0,
        }
    }

    #[inline]
    pub fn update_time(&mut self, delta_time_s: f32, total_time_s: f32) {
        self.delta_time_s = delta_time_s;
        self.total_time_s = total_time_s;
    }

    #[inline]
    pub fn next_frame(&mut self) {
        self.frame_counter.next_frame();
    }

    #[inline]
    pub fn frame_id(&self) -> u64 {
        self.frame_counter.frame_id()
    }

    #[inline]
    pub fn frame_label(&self) -> FrameLabel {
        self.frame_counter.frame_label()
    }

    #[inline]
    pub fn frame_token(&self) -> FrameToken {
        self.frame_counter.frame_token()
    }

    #[inline]
    pub fn frame_delta_time_limit_us(&self) -> f32 {
        self.frame_counter.frame_delta_time_limit_us()
    }

    #[inline]
    pub fn delta_time_s(&self) -> f32 {
        self.delta_time_s
    }

    #[inline]
    pub fn total_time_s(&self) -> f32 {
        self.total_time_s
    }

    #[inline]
    pub fn frame_counter(&self) -> &FrameCounter {
        &self.frame_counter
    }
}
