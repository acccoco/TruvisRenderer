use std::path::PathBuf;

use ash::vk;
use crossbeam_channel::{Receiver, Sender};
use crossbeam_utils::sync::WaitGroup;
use image::GenericImageView;

use crate::handle::{AssetTextureHandle, LoadedTextureBytes};

pub struct AssetLoadRequest {
    pub path: PathBuf,
    pub handle: AssetTextureHandle,
    // pub params: AssetParams, // 预留扩展
}

pub enum LoadResult {
    Success {
        handle: AssetTextureHandle,
        data: LoadedTextureBytes,
    },
    Failure(AssetTextureHandle, String),
}

/// 负责管理后台 IO/解码任务。
pub struct AssetLoader {
    pool: rayon::ThreadPool,
    result_sender: Sender<LoadResult>,
    result_receiver: Receiver<LoadResult>,
    wait_group: Option<WaitGroup>,
}

impl Default for AssetLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetLoader {
    pub fn new() -> Self {
        let (res_tx, res_rx) = crossbeam_channel::unbounded::<LoadResult>();

        let pool = rayon::ThreadPoolBuilder::new()
            .thread_name(|index| format!("Asset-Loader-{}", index))
            .build()
            .expect("Failed to create asset loader thread pool");

        Self {
            pool,
            result_sender: res_tx,
            result_receiver: res_rx,
            wait_group: Some(WaitGroup::new()),
        }
    }

    pub fn request_load(&self, req: AssetLoadRequest) {
        let result_sender = self.result_sender.clone();
        let wg_task = self.wait_group.as_ref().expect("AssetLoader used after drop").clone();
        self.pool.spawn(move || {
            let result = load_texture_task(req);
            let _ = result_sender.send(result);
            drop(wg_task);
        });
    }

    pub fn try_recv_result(&self) -> Option<LoadResult> {
        self.result_receiver.try_recv().ok()
    }
}

impl Drop for AssetLoader {
    fn drop(&mut self) {
        log::info!("AssetLoader is being dropped, waiting for tasks to complete...");
        if let Some(wait_group) = self.wait_group.take() {
            wait_group.wait();
        }
        log::info!("All asset loading tasks completed.");
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

            let data = LoadedTextureBytes {
                pixels,
                extent: vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                },
                format: vk::Format::R8G8B8A8_UNORM, // 目前统一转为 RGBA8
            };

            LoadResult::Success {
                handle: req.handle,
                data,
            }
        }
        Err(e) => {
            log::error!("Failed to load texture {:?}: {}", req.path, e);
            LoadResult::Failure(req.handle, e.to_string())
        }
    }
}
