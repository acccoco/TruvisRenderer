/// runtime 帧生命周期使用的单线程计时器。
///
/// `RenderRuntime::begin_frame` 在每帧开始调用 `tick`，随后 update/render 阶段读取
/// delta/total time 写入 `FrameTiming` 与 per-frame GPU 数据。它不负责限帧策略，
/// `time_to_render` 只是用当前 tick 后经过的时间与 `FrameCounter` 的期望间隔比较。
#[derive(Debug)]
pub(crate) struct FrameTimer {
    /// 记录计时器创建时刻，保留给后续需要真实 wall-clock 总时长的场景。
    _start_time: std::time::Instant,
    last_tick: std::time::Instant,

    delta_time: std::time::Duration,
    total_time: std::time::Duration,
}

impl Default for FrameTimer {
    fn default() -> Self {
        let now = std::time::Instant::now();
        Self {
            _start_time: now,
            last_tick: now,
            delta_time: std::time::Duration::ZERO,
            total_time: std::time::Duration::ZERO,
        }
    }
}

impl FrameTimer {
    /// 标记新帧开始，并推进上一帧 delta 与累计运行时间。
    pub(crate) fn tick(&mut self) {
        let now = std::time::Instant::now();
        self.delta_time = now.duration_since(self.last_tick);
        self.last_tick = now;
        self.total_time += self.delta_time;
    }

    /// 返回本帧 `tick` 之后已经经过的时间，用于判断是否到达下一次渲染时机。
    pub(crate) fn elapsed_since_tick(&self) -> std::time::Duration {
        self.last_tick.elapsed()
    }

    /// 上一帧间隔，单位毫秒；会写入 shader 侧 per-frame 数据。
    #[inline]
    pub(crate) fn delta_time_ms(&self) -> f32 {
        self.delta_time.as_secs_f32() * 1000.0
    }

    /// 上一帧间隔，单位秒；用于 CPU update 阶段。
    #[inline]
    pub(crate) fn delta_time_s(&self) -> f32 {
        self.delta_time.as_secs_f32()
    }

    /// 自计时器创建以来、按 `tick` 累加的运行时间，单位秒。
    #[inline]
    pub(crate) fn total_time_s(&self) -> f32 {
        self.total_time.as_secs_f32()
    }

    /// 自计时器创建以来、按 `tick` 累加的运行时间，单位毫秒。
    #[inline]
    pub(crate) fn total_time_ms(&self) -> f32 {
        self.total_time.as_secs_f32() * 1000.0
    }
}
