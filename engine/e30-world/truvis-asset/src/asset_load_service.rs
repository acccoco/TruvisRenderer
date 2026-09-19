use std::collections::VecDeque;
use std::sync::Arc;

use slotmap::SlotMap;

use crate::asset_load_worker::{AssetLoadWorker, LoadResult, SceneLoadRequest, TextureLoadRequest};
use crate::handle::{RawSceneData, SceneLoadDesc, SceneLoadHandle, TextureBytes, TextureLoadDesc, TextureLoadHandle};

/// `AssetLoadService` 内部的 texture loader task 记录。
///
/// 这里只保存本次 load desc，用于完成时生成自包含事件。任务完成后 record 立即移除。
pub(crate) struct TextureLoadRecord {
    pub(crate) desc: TextureLoadDesc,
}

/// `AssetLoadService` 内部的 scene loader task 记录。
///
/// 这里不保存 scene CPU data。完整 owned CPU scene payload 只通过
/// `AssetLoadEvent::SceneLoaded` 一次性交给 `AssetSystem`。
pub(crate) struct SceneLoadRecord {
    pub(crate) desc: SceneLoadDesc,
}

/// asset 层向外发布的 CPU ready / failed 事件。
///
/// 事件携带一次性 CPU payload 或失败原因；`AssetLoadService` 不作为长期 asset database，
/// 也不把 scene 拆成 mesh/material 内容资产。
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
    /// scene / prefab CPU 导入完成。
    SceneLoaded {
        handle: SceneLoadHandle,
        desc: SceneLoadDesc,
        data: RawSceneData,
    },
    /// scene / prefab CPU 导入失败。
    SceneFailed {
        handle: SceneLoadHandle,
        desc: SceneLoadDesc,
        error: String,
    },
}

/// 一次性 CPU asset loader service。
///
/// `AssetLoadService` 只负责创建 loader task、收集后台结果并生成事件。长期 scene identity、
/// texture 去重、scene ingest 协调和 GPU 资源上传都由 `AssetSystem` / render runtime 负责。
pub struct AssetLoadService {
    textures: SlotMap<TextureLoadHandle, TextureLoadRecord>,
    scenes: SlotMap<SceneLoadHandle, SceneLoadRecord>,
    pending_events: VecDeque<AssetLoadEvent>,
    loader: AssetLoadWorker,
}

impl Default for AssetLoadService {
    fn default() -> Self {
        Self::new()
    }
}

// 创建与初始化
impl AssetLoadService {
    /// 创建空的 loader service。
    pub fn new() -> Self {
        let _span = tracy_client::span!("AssetLoadService::new");

        Self {
            textures: SlotMap::with_key(),
            scenes: SlotMap::with_key(),
            pending_events: VecDeque::new(),
            loader: AssetLoadWorker::new(),
        }
    }
}

// 销毁
impl AssetLoadService {
    /// 消耗资产中心。
    ///
    /// 当前 asset 层没有额外显式释放逻辑；真正需要等待的是内部 `AssetLoadWorker` 的
    /// `Drop`，它会在 hub 被消费后等待后台任务结束。
    pub fn destroy(self) {}
}

// 对外接口
impl AssetLoadService {
    /// 请求加载纹理。
    ///
    /// 每次调用都创建一个独立 loader handle；同一 scene texture 是否复用由
    /// `AssetSystem` 按 scene texture key 决定。
    pub fn request_texture(&mut self, desc: TextureLoadDesc) -> TextureLoadHandle {
        let _span = tracy_client::span!("AssetLoadService::request_texture");
        let handle = self.textures.insert(TextureLoadRecord { desc: desc.clone() });

        log::info!("Request load texture: {}", desc.source_label());
        self.loader.request_load_texture(TextureLoadRequest { desc, handle });

        handle
    }

    /// 请求从文件读取并解码纹理的 task。
    pub fn request_texture_path(
        &mut self,
        path: impl Into<std::path::PathBuf>,
        color_space: crate::handle::TextureColorSpace,
    ) -> TextureLoadHandle {
        self.request_texture(TextureLoadDesc::File {
            path: path.into(),
            color_space,
        })
    }

    /// 请求从已拥有的 encoded bytes 解码纹理的 task。
    ///
    /// `AssetLoadService` 只持有 task 期间的 `Arc`；解码完成后 payload 通过事件交给 CPU
    /// `AssetStore`，不会在 hub 内形成长期像素 registry。
    pub fn request_texture_bytes(
        &mut self,
        identity: crate::handle::EmbeddedTextureId,
        bytes: Arc<[u8]>,
        mime_type: Option<String>,
        color_space: crate::handle::TextureColorSpace,
    ) -> TextureLoadHandle {
        self.request_texture(TextureLoadDesc::Embedded {
            identity,
            bytes,
            mime_type,
            color_space,
        })
    }

    /// 请求后台导入 scene / prefab。
    ///
    /// 每次调用都创建一个独立 loader handle；完成后只通过 `SceneLoaded` / `SceneFailed`
    /// 事件交付 CPU payload 或错误文本。
    pub fn request_scene(&mut self, desc: SceneLoadDesc) -> SceneLoadHandle {
        let _span = tracy_client::span!("AssetLoadService::request_scene");
        let handle = self.scenes.insert(SceneLoadRecord { desc: desc.clone() });

        log::info!("Request load scene: {:?}", desc.path);
        self.loader.request_load_scene(SceneLoadRequest { desc, handle });

        handle
    }

    /// 收集后台加载任务完成事件。
    ///
    /// 该函数是后台 loader 和 `GameWorld` 之间的同步点。返回后的事件队列已经被消费，
    /// `AssetLoadService` 不会再次重放同一事件。
    pub fn update(&mut self) -> Vec<AssetLoadEvent> {
        let _span = tracy_client::span!("AssetLoadService::update");
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
                        log::error!("AssetLoadService: completed unknown texture load handle {:?}", handle);
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
                        log::error!("AssetLoadService: failed unknown texture load handle {:?}", handle);
                    }
                }
                LoadResult::SceneSuccess { handle, data } => {
                    if let Some(record) = self.scenes.remove(handle) {
                        events.push(AssetLoadEvent::SceneLoaded {
                            handle,
                            desc: record.desc,
                            data,
                        });
                    } else {
                        log::error!("AssetLoadService: completed unknown scene load handle {:?}", handle);
                    }
                }
                LoadResult::SceneFailure(handle, error) => {
                    if let Some(record) = self.scenes.remove(handle) {
                        events.push(AssetLoadEvent::SceneFailed {
                            handle,
                            desc: record.desc,
                            error,
                        });
                    } else {
                        log::error!("AssetLoadService: failed unknown scene load handle {:?}", handle);
                    }
                }
            }
        }

        events
    }
}
