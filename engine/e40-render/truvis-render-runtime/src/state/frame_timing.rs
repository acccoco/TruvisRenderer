use std::time::{Duration, Instant};

use truvis_render_foundation::frame_label::FrameLabel;

/// `RenderRuntime` 独占的帧序号、时间快照与限帧状态。
///
/// App 和 render pass 只读取同一帧在 `begin_frame` 采样出的稳定快照；帧号只在
/// present 或空帧 timeline signal 完成后由 `next_frame` 推进。
pub struct FrameTiming {
    frame_id: u64,
    started_at: Instant,
    last_frame_started_at: Instant,
    delta_time: Duration,
    total_time: Duration,
    min_frame_interval: Option<Duration>,
}

impl FrameTiming {
    pub(crate) fn new(frame_id: u64, min_frame_interval: Option<Duration>) -> Self {
        let now = Instant::now();
        Self {
            frame_id,
            started_at: now,
            last_frame_started_at: now,
            delta_time: Duration::ZERO,
            total_time: Duration::ZERO,
            min_frame_interval,
        }
    }

    /// 使用同一个时钟采样点更新本帧 delta、total 和下一帧限帧锚点。
    pub(crate) fn begin_frame(&mut self) {
        let now = Instant::now();
        self.delta_time = now.duration_since(self.last_frame_started_at);
        self.total_time = now.duration_since(self.started_at);
        self.last_frame_started_at = now;
    }

    #[inline]
    pub(crate) fn next_frame(&mut self) {
        self.frame_id = self.frame_id.wrapping_add(1);
    }

    #[inline]
    pub(crate) fn time_to_render(&self) -> bool {
        self.min_frame_interval
            .is_none_or(|min_frame_interval| self.last_frame_started_at.elapsed() >= min_frame_interval)
    }

    #[inline]
    pub fn frame_id(&self) -> u64 {
        self.frame_id
    }

    #[inline]
    pub fn frame_label(&self) -> FrameLabel {
        FrameLabel::from_frame_id(self.frame_id)
    }

    #[inline]
    pub fn delta_time_s(&self) -> f32 {
        self.delta_time.as_secs_f32()
    }

    #[inline]
    pub fn delta_time_ms(&self) -> f32 {
        self.delta_time_s() * 1000.0
    }

    #[inline]
    pub fn total_time_s(&self) -> f32 {
        self.total_time.as_secs_f32()
    }

    #[inline]
    pub fn total_time_ms(&self) -> f32 {
        self.total_time_s() * 1000.0
    }
}
