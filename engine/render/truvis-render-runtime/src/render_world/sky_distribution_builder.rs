use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use truvis_asset::handle::TextureBytes;
use truvis_shader_binding::gpu;
use truvis_world::guid_new_type::TextureHandle;

const MAX_DISTRIBUTION_WIDTH: u32 = 4096;
const MAX_DISTRIBUTION_HEIGHT: u32 = 2048;

/// 一次 sky distribution 构建请求的稳定身份。
///
/// `request_id` 由 render thread 单调递增；worker 不解释 scene 状态，只把 id 与
/// texture handle 原样带回，使 CPU/GPU completion 都能在发布前做 generation 校验。
pub(crate) struct SkyDistributionBuildRequest {
    pub(crate) request_id: u64,
    pub(crate) texture: TextureHandle,
    pub(crate) texture_bytes: TextureBytes,
}

/// 可提交到共享 transfer queue 的 Alias distribution。
pub(crate) struct SkyDistributionBuild {
    pub(crate) request_id: u64,
    pub(crate) texture: TextureHandle,
    pub(crate) source_width: u32,
    pub(crate) source_height: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) entries: Vec<gpu::scene::SkyDistributionEntry>,
    pub(crate) cpu_build_elapsed: Duration,
}

/// worker 完成结果。
///
/// 零能量纹理显式返回 `UniformFallback`，render thread 因而不会创建空 Alias buffer，
/// 但仍可记录本次 request 已完成并继续使用 1x1 uniform sphere distribution。
pub(crate) enum SkyDistributionBuildResult {
    Ready(SkyDistributionBuild),
    UniformFallback {
        request_id: u64,
        texture: TextureHandle,
        source_width: u32,
        source_height: u32,
        cpu_build_elapsed: Duration,
    },
}

enum SkyDistributionWorkerCommand {
    Build(SkyDistributionBuildRequest),
    Shutdown,
}

/// 单线程 sky distribution CPU builder。
///
/// render thread 同时只派发一个 in-flight build；如果用户连续切换天空，manager 只保留
/// 最新 pending request。该节流规则把 4K Alias 构建的内存峰值限制为单份工作集。
pub(crate) struct SkyDistributionBuilder {
    request_sender: Sender<SkyDistributionWorkerCommand>,
    result_receiver: Receiver<SkyDistributionBuildResult>,
    worker: Option<JoinHandle<()>>,
    in_flight: bool,
    latest_pending: Option<SkyDistributionBuildRequest>,
    destroyed: bool,
}

impl SkyDistributionBuilder {
    pub(crate) fn new() -> Self {
        let (request_sender, request_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("SkyDistributionBuilder".to_string())
            .spawn(move || {
                while let Ok(command) = request_receiver.recv() {
                    match command {
                        SkyDistributionWorkerCommand::Build(request) => {
                            let result = build_distribution(request);
                            if result_sender.send(result).is_err() {
                                break;
                            }
                        }
                        SkyDistributionWorkerCommand::Shutdown => break,
                    }
                }
            })
            .expect("failed to spawn SkyDistributionBuilder worker");

        Self {
            request_sender,
            result_receiver,
            worker: Some(worker),
            in_flight: false,
            latest_pending: None,
            destroyed: false,
        }
    }

    /// 请求构建；已有 build 运行时只覆盖 pending，不并行分配第二份大型工作集。
    pub(crate) fn request(&mut self, request: SkyDistributionBuildRequest) {
        if self.in_flight {
            self.latest_pending = Some(request);
            return;
        }
        self.dispatch(request);
    }

    /// 非阻塞收集完成结果，并在 worker 空闲后立即派发最新 pending request。
    pub(crate) fn poll(&mut self) -> Vec<SkyDistributionBuildResult> {
        let mut completed = Vec::new();
        loop {
            match self.result_receiver.try_recv() {
                Ok(result) => {
                    self.in_flight = false;
                    completed.push(result);
                    if let Some(request) = self.latest_pending.take() {
                        self.dispatch(request);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.in_flight = false;
                    break;
                }
            }
        }
        completed
    }

    /// 停止 CPU producer 并 join worker。
    ///
    /// 已经运行的单个 build 会先自然完成；未派发的 latest pending 直接丢弃。调用者应在
    /// 此后再关闭 GPU transfer queue，保证不会与 producer 竞态产生新 submission。
    pub(crate) fn shutdown(&mut self) {
        if self.destroyed {
            return;
        }
        self.latest_pending = None;
        let _ = self.request_sender.send(SkyDistributionWorkerCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                log::error!("SkyDistributionBuilder worker panicked during shutdown");
            }
        }
        self.in_flight = false;
        self.destroyed = true;
    }

    fn dispatch(&mut self, request: SkyDistributionBuildRequest) {
        match self.request_sender.send(SkyDistributionWorkerCommand::Build(request)) {
            Ok(()) => self.in_flight = true,
            Err(error) => {
                self.in_flight = false;
                log::error!("SkyDistributionBuilder request channel disconnected: {error}");
            }
        }
    }
}

impl Drop for SkyDistributionBuilder {
    fn drop(&mut self) {
        debug_assert!(self.destroyed, "SkyDistributionBuilder dropped without explicit shutdown");
    }
}

fn build_distribution(request: SkyDistributionBuildRequest) -> SkyDistributionBuildResult {
    let started_at = Instant::now();
    let extent = request.texture_bytes.extent();
    let source_width = extent.width;
    let source_height = extent.height;
    if source_width != source_height.saturating_mul(2) {
        log::warn!(
            "SkyDistributionBuilder: texture {:?} is {}x{}, expected a 2:1 lat-long environment map",
            request.texture,
            source_width,
            source_height
        );
    }

    let Some((width, height)) = distribution_extent(source_width, source_height) else {
        return SkyDistributionBuildResult::UniformFallback {
            request_id: request.request_id,
            texture: request.texture,
            source_width,
            source_height,
            cpu_build_elapsed: started_at.elapsed(),
        };
    };
    let entry_count = width as usize * height as usize;
    let mut scaled_weights = vec![0.0_f64; entry_count];

    // 每个源 texel 以 UV 中心归入目标 cell，并累积真实 source solid angle。
    // 这不是普通图像缩放：小面积高亮太阳即使落入较低分辨率 cell，能量也不会消失。
    for source_y in 0..source_height {
        let source_solid_angle = lat_long_texel_solid_angle(source_y, source_width, source_height);
        let target_y = ((((source_y as f64 + 0.5) * height as f64) / source_height as f64) as u32).min(height - 1);
        for source_x in 0..source_width {
            let source_index = source_y as usize * source_width as usize + source_x as usize;
            let [r, g, b] = request.texture_bytes.linear_rgb(source_index);
            let luminance = 0.2126_f64 * r as f64 + 0.7152_f64 * g as f64 + 0.0722_f64 * b as f64;
            if !luminance.is_finite() || luminance <= 0.0 {
                continue;
            }
            let target_x = ((((source_x as f64 + 0.5) * width as f64) / source_width as f64) as u32).min(width - 1);
            let target_index = target_y as usize * width as usize + target_x as usize;
            scaled_weights[target_index] += luminance * source_solid_angle;
        }
    }

    let total_weight: f64 = scaled_weights.iter().sum();
    if !total_weight.is_finite() || total_weight <= f64::EPSILON {
        return SkyDistributionBuildResult::UniformFallback {
            request_id: request.request_id,
            texture: request.texture,
            source_width,
            source_height,
            cpu_build_elapsed: started_at.elapsed(),
        };
    }

    // entries 先直接写入最终 solid-angle PDF，随后复用唯一的 f64 Vec 原地保存
    // Alias scaled probability；不保留 solid_angles/probability/index 的平行大数组。
    let mut entries = Vec::with_capacity(entry_count);
    for (index, weight) in scaled_weights.iter().copied().enumerate() {
        let row = index as u32 / width;
        let cell_solid_angle = lat_long_texel_solid_angle(row, width, height);
        let solid_angle_pdf =
            if cell_solid_angle > 0.0 { (weight / total_weight / cell_solid_angle) as f32 } else { 0.0 };
        entries.push(gpu::scene::SkyDistributionEntry {
            alias_probability: 1.0,
            solid_angle_pdf,
            alias_index: index as u32,
            _padding_0: 0,
        });
    }

    let entry_count_f64 = entry_count as f64;
    for weight in &mut scaled_weights {
        *weight = *weight * entry_count_f64 / total_weight;
    }
    let mut small: Vec<u32> = Vec::new();
    let mut large: Vec<u32> = Vec::new();
    for (index, probability) in scaled_weights.iter().copied().enumerate() {
        if probability < 1.0 {
            small.push(index as u32);
        } else {
            large.push(index as u32);
        }
    }

    while !small.is_empty() && !large.is_empty() {
        let small_index = small.pop().unwrap();
        let large_index = large.pop().unwrap();
        entries[small_index as usize].alias_probability = scaled_weights[small_index as usize].clamp(0.0, 1.0) as f32;
        entries[small_index as usize].alias_index = large_index;

        scaled_weights[large_index as usize] += scaled_weights[small_index as usize] - 1.0;
        if scaled_weights[large_index as usize] < 1.0 {
            small.push(large_index);
        } else {
            large.push(large_index);
        }
    }
    for index in small.into_iter().chain(large) {
        entries[index as usize].alias_probability = 1.0;
        entries[index as usize].alias_index = index;
    }

    SkyDistributionBuildResult::Ready(SkyDistributionBuild {
        request_id: request.request_id,
        texture: request.texture,
        source_width,
        source_height,
        width,
        height,
        entries,
        cpu_build_elapsed: started_at.elapsed(),
    })
}

fn distribution_extent(source_width: u32, source_height: u32) -> Option<(u32, u32)> {
    if source_width == 0 || source_height == 0 {
        return None;
    }
    if source_width <= MAX_DISTRIBUTION_WIDTH && source_height <= MAX_DISTRIBUTION_HEIGHT {
        return Some((source_width, source_height));
    }

    let scale = (MAX_DISTRIBUTION_WIDTH as f64 / source_width as f64)
        .min(MAX_DISTRIBUTION_HEIGHT as f64 / source_height as f64);
    let width = ((source_width as f64 * scale).floor() as u32).clamp(1, MAX_DISTRIBUTION_WIDTH);
    let height = ((source_height as f64 * scale).floor() as u32).clamp(1, MAX_DISTRIBUTION_HEIGHT);
    Some((width, height))
}

fn lat_long_texel_solid_angle(row: u32, width: u32, height: u32) -> f64 {
    let dphi = 2.0 * std::f64::consts::PI / f64::from(width);
    let v0 = f64::from(row) / f64::from(height);
    let v1 = f64::from(row + 1) / f64::from(height);
    let theta_top = (0.5 - v0) * std::f64::consts::PI;
    let theta_bottom = (0.5 - v1) * std::f64::consts::PI;
    dphi * (theta_top.sin() - theta_bottom.sin()).max(0.0)
}
