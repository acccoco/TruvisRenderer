use crossbeam_channel::{Receiver, Sender};
use crossbeam_utils::sync::WaitGroup;

use crate::gltf_scene_loader::load_gltf_scene_task;
use crate::handle::{ModelLoadDesc, ModelLoadHandle, RawSceneData, TextureBytes, TextureLoadDesc, TextureLoadHandle};
use crate::texture_loader::load_texture_task;
use crate::truvixx_scene_loader::load_scene_task;

/// 纹理加载请求。
///
/// 请求由 `AssetHub::request_texture` 构造。后台任务只使用 desc 读取 CPU 数据、
/// 使用 handle 回传结果，
/// 不直接访问 hub 状态表。
pub(crate) struct TextureLoadRequest {
    pub desc: TextureLoadDesc,
    pub handle: TextureLoadHandle,
}

/// model / prefab 导入请求。
///
/// path 是导入源。后台任务只负责读取和复制 CPU 数据，raw index 到 scene handle 的
/// 映射由 `SceneAssetIngestor` 在 asset sync 阶段完成。
pub(crate) struct ModelLoadRequest {
    pub desc: ModelLoadDesc,
    pub handle: ModelLoadHandle,
}

/// 后台任务回传给 `AssetHub::update()` 的 CPU 加载结果。
///
/// 结果中只携带 owned Rust 数据或错误文本，不携带 C++ scene handle、raw pointer
/// 或任何 GPU 资源。
pub(crate) enum LoadResult {
    TextureSuccess {
        handle: TextureLoadHandle,
        data: TextureBytes,
    },
    TextureFailure(TextureLoadHandle, String),
    ModelSuccess {
        handle: ModelLoadHandle,
        data: RawSceneData,
    },
    ModelFailure(ModelLoadHandle, String),
}

/// 负责管理 asset 后台 IO、纹理解码和 model 导入任务。
///
/// `AssetLoader` 隐藏 Rayon 线程池和结果 channel。外部只通过 `AssetHub`
/// 轮询结果，因此后台线程不会直接修改 asset 状态表，也不会接触渲染运行时 GPU 对象。
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

    /// 排队一个 model 导入任务。
    ///
    /// 导入任务会在后台按文件格式选择 Assimp 或 glTF loader，并在返回前复制出 owned
    /// Rust 数据。loader result 到 scene handle 的转换由 `SceneAssetIngestor` 完成。
    pub(crate) fn request_load_model(&self, req: ModelLoadRequest) {
        let result_sender = self.result_sender.clone();
        let wg_task = self.wait_group.as_ref().expect("AssetLoader used after drop").clone();
        self.pool.spawn(move || {
            let is_gltf = Self::is_gltf_path(&req.desc.path);
            let result = if is_gltf { load_gltf_scene_task(req) } else { load_scene_task(req) };
            let _ = result_sender.send(result);
            drop(wg_task);
        });
    }

    /// 判断 model 导入请求是否应走 Rust glTF loader。
    ///
    /// 这里只按扩展名做大小写无关分派；非 glTF 格式继续保持原有 Assimp 路径。
    fn is_gltf_path(path: &std::path::Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("gltf") || ext.eq_ignore_ascii_case("glb"))
            .unwrap_or(false)
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
