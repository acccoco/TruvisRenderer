use std::path::PathBuf;

use crossbeam_channel::{Receiver, Sender};
use crossbeam_utils::sync::WaitGroup;

use crate::handle::{AssetSceneHandle, AssetTextureHandle, RawSceneData, TextureBytes};
use crate::texture_loader::load_texture_task;
use crate::truvixx_scene_loader::load_scene_task;

/// 纹理加载请求。
///
/// 请求由 `AssetHub::load_texture` 构造，handle 已经在 hub 中分配并进入
/// `Loading` 状态。后台任务只使用该 handle 回传结果，不直接访问 hub 状态表。
pub(crate) struct TextureLoadRequest {
    pub path: PathBuf,
    pub handle: AssetTextureHandle,
}

/// scene / prefab 导入请求。
///
/// path 是导入源，也是后续 scene、mesh、material key 的来源。后台任务只负责读取和
/// 复制 CPU 数据，raw index 到 asset handle 的映射由 `AssetHub::update()` 完成。
pub(crate) struct SceneLoadRequest {
    pub path: PathBuf,
    pub handle: AssetSceneHandle,
}

/// 后台任务回传给 `AssetHub::update()` 的 CPU 加载结果。
///
/// 结果中只携带 owned Rust 数据或错误文本，不携带 C++ scene handle、raw pointer
/// 或任何 GPU 资源。
pub(crate) enum LoadResult {
    TextureSuccess {
        handle: AssetTextureHandle,
        data: TextureBytes,
    },
    TextureFailure(AssetTextureHandle, String),
    SceneSuccess {
        handle: AssetSceneHandle,
        data: RawSceneData,
    },
    SceneFailure(AssetSceneHandle, String),
}

/// 负责管理 asset 后台 IO、纹理解码和 scene 导入任务。
///
/// `AssetLoader` 隐藏 Rayon 线程池和结果 channel。外部只通过 `AssetHub`
/// 轮询结果，因此后台线程不会直接修改 asset 状态表，也不会接触渲染后端 GPU 对象。
pub(crate) struct AssetLoader {
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
    /// 创建后台 asset loader。
    ///
    /// loader 拥有独立 Rayon 线程池和无界结果 channel。结果 channel 只在
    /// `AssetHub::update()` 中轮询，因此所有 asset 状态变更都收敛到调用线程。
    pub(crate) fn new() -> Self {
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

    /// 排队一个纹理加载任务。
    ///
    /// 任务在 Rayon worker 上执行文件读取和 image 解码。完成后只发送 `LoadResult`，
    /// 不修改 `AssetHub`，也不创建任何 Vulkan 对象。
    pub(crate) fn request_load_texture(&self, req: TextureLoadRequest) {
        let result_sender = self.result_sender.clone();
        let wg_task = self.wait_group.as_ref().expect("AssetLoader used after drop").clone();
        self.pool.spawn(move || {
            let result = load_texture_task(req);
            let _ = result_sender.send(result);
            drop(wg_task);
        });
    }

    /// 排队一个 scene 导入任务。
    ///
    /// 导入任务会在后台持有 C++ scene handle，并在返回前复制出 owned Rust 数据。
    /// handle/key 分配和事件生成仍由 `AssetHub` 在 `update()` 中完成。
    pub(crate) fn request_load_scene(&self, req: SceneLoadRequest) {
        let result_sender = self.result_sender.clone();
        let wg_task = self.wait_group.as_ref().expect("AssetLoader used after drop").clone();
        self.pool.spawn(move || {
            let result = load_scene_task(req);
            let _ = result_sender.send(result);
            drop(wg_task);
        });
    }

    /// 非阻塞读取一个后台任务结果。
    ///
    /// 返回 `None` 表示当前没有完成结果；调用方应在帧循环或显式同步点继续轮询。
    pub(crate) fn try_recv_result(&self) -> Option<LoadResult> {
        self.result_receiver.try_recv().ok()
    }
}

impl Drop for AssetLoader {
    /// 等待已经排队的后台任务结束。
    ///
    /// 这保证 `AssetHub` 销毁时不会留下仍在访问请求数据或 C++ importer 的 worker。
    /// 等待只发生在 loader drop；正常帧同步仍应通过 `try_recv_result` 非阻塞收集结果。
    fn drop(&mut self) {
        log::info!("AssetLoader is being dropped, waiting for tasks to complete...");
        if let Some(wait_group) = self.wait_group.take() {
            wait_group.wait();
        }
        log::info!("All asset loading tasks completed.");
    }
}
