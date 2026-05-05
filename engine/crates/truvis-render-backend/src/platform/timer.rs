#[derive(Debug)]
pub struct Timer {
    _start_time: std::time::Instant,
    last_tick: std::time::Instant,

    delta_time: std::time::Duration,
    total_time: std::time::Duration,
}

impl Default for Timer {
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

impl Timer {
    /// 每帧开始的时候调用
    pub fn tick(&mut self) {
        let now = std::time::Instant::now();
        self.delta_time = now.duration_since(self.last_tick);
        self.last_tick = now;
        self.total_time += self.delta_time;
    }

    pub fn elapsed_since_tick(&self) -> std::time::Duration {
        self.last_tick.elapsed()
    }

    #[inline]
    pub fn delta_time(&self) -> std::time::Duration {
        self.delta_time
    }

    /// 上一帧的时间（毫秒）
    #[inline]
    pub fn delta_time_ms(&self) -> f32 {
        self.delta_time.as_secs_f32() * 1000.0
    }

    /// 上一帧的时间（秒）
    #[inline]
    pub fn delta_time_s(&self) -> f32 {
        self.delta_time.as_secs_f32()
    }

    /// 当前帧率
    #[inline]
    pub fn fps(&self) -> f32 {
        1.0 / self.delta_time.as_secs_f32()
    }

    /// 总运行时间
    #[inline]
    pub fn total_time_s(&self) -> f32 {
        self.total_time.as_secs_f32()
    }

    #[inline]
    pub fn total_time_ms(&self) -> f32 {
        self.total_time.as_secs_f32() * 1000.0
    }
}
