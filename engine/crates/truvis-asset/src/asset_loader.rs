use std::path::PathBuf;
use std::thread;

use ash::vk;
use crossbeam_channel::{Receiver, Sender};
use crossbeam_utils::sync::WaitGroup;
use image::GenericImageView;

use crate::handle::AssetTextureHandle;

pub struct AssetLoadRequest {
    pub path: PathBuf,
    pub handle: AssetTextureHandle,
    // pub params: AssetParams, // Future expansion
}

/// 解码后的原始资产数据 (CPU 端)
/// 准备好上传到 GPU
pub struct RawAssetData {
    pub pixels: Vec<u8>,
    pub extent: vk::Extent3D,
    pub format: vk::Format,
    pub handle: AssetTextureHandle,
    pub mip_levels: u32,
}

pub enum LoadResult {
    Success(RawAssetData),
    Failure(AssetTextureHandle, String),
}

/// 负责管理后台 IO 任务。
///
/// ## 架构设计
/// - 内部的 `dispatch-thread` 负责调度：接收加载请求，分发任务到 workder
/// - rayon 提供 worker 线程池
/// - crossbeam 提供线程间通信的 channel
/// - 外部线程和 dispatch-thread 之间的通信
///     - request_rx: 接收加载请求
///     - request_tx: 发送加载请求
/// - dispatch_thread 和 workder 线程池的通信
///     - result_tx: 发送加载结果
///     - result_rx: 接收加载结果
pub struct AssetLoader {
    /// 用于向 IoWorker 发送加载请求
    request_sender: Option<Sender<AssetLoadRequest>>,
    /// 用于从 IoWorker 接收加载结果
    result_receiver: Receiver<LoadResult>,

    /// 用于分发 IO 任务的后台线程
    dispatch_thread: Option<std::thread::JoinHandle<()>>,
}

impl Default for AssetLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetLoader {
    pub fn new() -> Self {
        let (req_tx, req_rx) = crossbeam_channel::unbounded::<AssetLoadRequest>();
        let (res_tx, res_rx) = crossbeam_channel::unbounded::<LoadResult>();

        // Rayon 线程池，用于执行实际的加载任务
        let pool = rayon::ThreadPoolBuilder::new()
            .thread_name(|index| format!("Asset-Loader-{}", index))
            .build()
            .expect("Failed to create asset loader thread pool");

        // 调度线程，负责接收请求并分发任务
        let dispatch_thread = thread::Builder::new()
            .name("AssetDispatchThread".to_string())
            .spawn(move || {
                let wait_group = WaitGroup::new();

                while let Ok(req) = req_rx.recv() {
                    let _span = tracy_client::span!("IoWorker::dispatch");

                    let res_tx = res_tx.clone();
                    // 为每个任务克隆一个 WaitGroup
                    // 当任务结束，闭包销毁，wg_task 也会被 drop
                    let wg_task = wait_group.clone();

                    // 使用专用线程池执行任务
                    pool.spawn(move || {
                        let result = load_texture_task(req);
                        let _ = res_tx.send(result);

                        // wg_task 在这里自动 drop
                        drop(wg_task);
                    });
                }

                // 等待所有任务完成
                wait_group.wait();
            })
            .expect("Failed to spawn IO dispatcher thread");

        Self {
            request_sender: Some(req_tx),
            result_receiver: res_rx,

            dispatch_thread: Some(dispatch_thread),
        }
    }

    pub fn request_load(&self, req: AssetLoadRequest) {
        if let Some(sender) = &self.request_sender
            && let Err(e) = sender.send(req)
        {
            log::error!("Failed to send asset load request: {}", e);
        }
    }

    pub fn try_recv_result(&self) -> Option<LoadResult> {
        self.result_receiver.try_recv().ok()
    }

    /// 显式等待所有任务完成并销毁 IoWorker
    /// 实际上只是消耗 self，触发 Drop
    pub fn join(self) {}
}

impl Drop for AssetLoader {
    fn drop(&mut self) {
        // 显式关闭 channel，通知后台线程退出
        // 必须先 drop sender，否则 recv 会一直阻塞，导致 join 死锁
        self.request_sender = None;

        log::info!("IoWorker is being dropped, waiting for tasks to complete...");
        if let Some(thread) = self.dispatch_thread.take()
            && let Err(_) = thread.join()
        {
            log::error!("Failed to join IO dispatcher thread");
        }
        log::info!("All IO tasks completed, IoWorker dropped.");
    }
}

/// 实际的加载任务 (运行在 Rayon 线程池中)
/// 执行: 文件读取 -> 图片解码 -> 格式转换
fn load_texture_task(req: AssetLoadRequest) -> LoadResult {
    let _span = tracy_client::span!("load_texture_task");
    log::info!("Loading texture: {:?}", req.path);

    let img_result = image::open(&req.path);

    match img_result {
        Ok(img) => {
            let (width, height) = img.dimensions();
            // 强制转换为 RGBA8
            let img = img.into_rgba8();
            let pixels = img.into_raw();

            let raw_data = RawAssetData {
                pixels,
                extent: vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                },
                format: vk::Format::R8G8B8A8_UNORM, // 目前统一转为 RGBA8
                handle: req.handle,
                mip_levels: 1, // 暂时只加载 level 0
            };

            LoadResult::Success(raw_data)
        }
        Err(e) => {
            log::error!("Failed to load texture {:?}: {}", req.path, e);
            LoadResult::Failure(req.handle, e.to_string())
        }
    }
}
