use std::collections::VecDeque;

use slotmap::SlotMap;

use crate::asset_loader::{AssetLoader, LoadResult, ModelLoadRequest, TextureLoadRequest};
use crate::handle::{ModelLoadDesc, ModelLoadHandle, RawSceneData, TextureBytes, TextureLoadDesc, TextureLoadHandle};

/// `AssetHub` 内部的 texture loader task 记录。
///
/// 这里只保存本次 load desc，用于完成时生成自包含事件。任务完成后 record 立即移除。
pub(crate) struct TextureLoadRecord {
    pub(crate) desc: TextureLoadDesc,
}

/// `AssetHub` 内部的 model loader task 记录。
///
/// 这里不保存 model CPU data。完整 owned CPU scene payload 只通过
/// `AssetLoadEvent::ModelLoaded` 一次性交给 `SceneAssetIngestor`。
pub(crate) struct ModelLoadRecord {
    pub(crate) desc: ModelLoadDesc,
}

/// asset 层向外发布的 CPU ready / failed 事件。
///
/// 事件携带一次性 CPU payload 或失败原因；`AssetHub` 不作为长期 asset database，
/// 也不把 model 拆成 mesh/material 内容资产。
#[derive(Debug)]
pub enum AssetLoadEvent {
    /// 纹理文件已经完成 CPU 解码。
    TextureLoaded {
        handle: TextureLoadHandle,
        desc: TextureLoadDesc,
        data: TextureBytes,
    },
    /// 纹理 CPU 加载或解码失败。
    TextureFailed {
        handle: TextureLoadHandle,
        desc: TextureLoadDesc,
        error: String,
    },
    /// model / prefab CPU 导入完成。
    ModelLoaded {
        handle: ModelLoadHandle,
        desc: ModelLoadDesc,
        data: RawSceneData,
    },
    /// model / prefab CPU 导入失败。
    ModelFailed {
        handle: ModelLoadHandle,
        desc: ModelLoadDesc,
        error: String,
    },
}

/// 一次性 CPU asset loader service。
///
/// `AssetHub` 只负责创建 loader task、收集后台结果并生成事件。长期 scene identity、
/// texture 去重、model ingest transaction 和 render upload payload 都由 `World` 内部的
/// `SceneAssetIngestor` / `SceneStore` 负责。
pub struct AssetHub {
    textures: SlotMap<TextureLoadHandle, TextureLoadRecord>,
    models: SlotMap<ModelLoadHandle, ModelLoadRecord>,
    pending_events: VecDeque<AssetLoadEvent>,
    loader: AssetLoader,
}

impl Default for AssetHub {
    fn default() -> Self {
        Self::new()
    }
}

// 创建与初始化
impl AssetHub {
    /// 创建空的 loader service。
    pub fn new() -> Self {
        let _span = tracy_client::span!("AssetHub::new");

        Self {
            textures: SlotMap::with_key(),
            models: SlotMap::with_key(),
            pending_events: VecDeque::new(),
            loader: AssetLoader::new(),
        }
    }
}

// 销毁
impl AssetHub {
    /// 消耗资产中心。
    ///
    /// 当前 asset 层没有额外显式释放逻辑；真正需要等待的是内部 `AssetLoader` 的
    /// `Drop`，它会在 hub 被消费后等待后台任务结束。
    pub fn destroy(self) {}
}

// 对外接口
impl AssetHub {
    /// 请求加载纹理。
    ///
    /// 每次调用都创建一个独立 loader handle；同一 scene texture 是否复用由
    /// `SceneAssetIngestor` 按 scene texture key 决定。
    pub fn request_texture(&mut self, desc: TextureLoadDesc) -> TextureLoadHandle {
        let _span = tracy_client::span!("AssetHub::request_texture");
        let handle = self.textures.insert(TextureLoadRecord { desc: desc.clone() });

        log::info!("Request load texture: {:?}", desc.path);
        self.loader.request_load_texture(TextureLoadRequest { desc, handle });

        handle
    }

    /// 请求后台导入 model / prefab。
    ///
    /// 每次调用都创建一个独立 loader handle；完成后只通过 `ModelLoaded` / `ModelFailed`
    /// 事件交付 CPU payload 或错误文本。
    pub fn request_model(&mut self, desc: ModelLoadDesc) -> ModelLoadHandle {
        let _span = tracy_client::span!("AssetHub::request_model");
        let handle = self.models.insert(ModelLoadRecord { desc: desc.clone() });

        log::info!("Request load model: {:?}", desc.path);
        self.loader.request_load_model(ModelLoadRequest { desc, handle });

        handle
    }

    /// 收集后台加载任务完成事件。
    ///
    /// 该函数是后台 loader 和 `World` 之间的同步点。返回后的事件队列已经被消费，
    /// `AssetHub` 不会再次重放同一事件。
    pub fn update(&mut self) -> Vec<AssetLoadEvent> {
        let _span = tracy_client::span!("AssetHub::update");
        let mut events = Vec::new();

        while let Some(event) = self.pending_events.pop_front() {
            events.push(event);
        }

        while let Some(result) = self.loader.try_recv_result() {
            match result {
                LoadResult::TextureSuccess { handle, data } => {
                    if let Some(record) = self.textures.remove(handle) {
                        events.push(AssetLoadEvent::TextureLoaded {
                            handle,
                            desc: record.desc,
                            data,
                        });
                    } else {
                        log::error!("AssetHub: completed unknown texture load handle {:?}", handle);
                    }
                }
                LoadResult::TextureFailure(handle, error) => {
                    if let Some(record) = self.textures.remove(handle) {
                        events.push(AssetLoadEvent::TextureFailed {
                            handle,
                            desc: record.desc,
                            error,
                        });
                    } else {
                        log::error!("AssetHub: failed unknown texture load handle {:?}", handle);
                    }
                }
                LoadResult::ModelSuccess { handle, data } => {
                    if let Some(record) = self.models.remove(handle) {
                        events.push(AssetLoadEvent::ModelLoaded {
                            handle,
                            desc: record.desc,
                            data,
                        });
                    } else {
                        log::error!("AssetHub: completed unknown model load handle {:?}", handle);
                    }
                }
                LoadResult::ModelFailure(handle, error) => {
                    if let Some(record) = self.models.remove(handle) {
                        events.push(AssetLoadEvent::ModelFailed {
                            handle,
                            desc: record.desc,
                            error,
                        });
                    } else {
                        log::error!("AssetHub: failed unknown model load handle {:?}", handle);
                    }
                }
            }
        }

        events
    }
}
