use std::collections::HashMap;
use std::path::{Path, PathBuf};

use slotmap::SlotMap;

use crate::asset_loader::{AssetLoadRequest, AssetLoader, LoadResult};
use crate::handle::{AssetTextureHandle, LoadStatus, LoadedTextureBytes};

pub struct TextureAssetRecord {
    pub path: PathBuf,
    pub status: LoadStatus,
}

pub enum LoadedAssetEvent {
    TextureLoaded {
        handle: AssetTextureHandle,
        data: LoadedTextureBytes,
    },
    TextureFailed {
        handle: AssetTextureHandle,
        error: String,
    },
}

/// 资产中心。
///
/// 只负责资产身份、路径去重和文件到 CPU bytes 的加载流程。
pub struct AssetHub {
    textures: SlotMap<AssetTextureHandle, TextureAssetRecord>,
    path_to_texture: HashMap<PathBuf, AssetTextureHandle>,
    loader: AssetLoader,
}

impl Default for AssetHub {
    fn default() -> Self {
        Self::new()
    }
}

// 创建与初始化
impl AssetHub {
    pub fn new() -> Self {
        Self {
            textures: SlotMap::with_key(),
            path_to_texture: HashMap::new(),
            loader: AssetLoader::new(),
        }
    }
}

// 销毁
impl AssetHub {
    pub fn destroy(self) {}
}

// 访问器
impl AssetHub {
    /// 请求加载纹理。
    ///
    /// 同一路径只分配一个稳定的 `AssetTextureHandle`。
    pub fn load_texture(&mut self, path: PathBuf) -> AssetTextureHandle {
        let _span = tracy_client::span!("AssetHub::load_texture");
        if let Some(&handle) = self.path_to_texture.get(&path) {
            return handle;
        }

        let handle = self.textures.insert(TextureAssetRecord {
            path: path.clone(),
            status: LoadStatus::Loading,
        });
        self.path_to_texture.insert(path.clone(), handle);

        log::info!("Request load texture: {:?}", path);
        self.loader.request_load(AssetLoadRequest { path, handle });

        handle
    }

    pub fn get_status(&self, handle: AssetTextureHandle) -> LoadStatus {
        self.textures.get(handle).map(|record| record.status).unwrap_or(LoadStatus::Failed)
    }

    pub fn texture_handle_by_path(&self, path: &Path) -> Option<AssetTextureHandle> {
        self.path_to_texture.get(path).copied()
    }

    /// 收集后台加载任务完成事件。
    pub fn update(&mut self) -> Vec<LoadedAssetEvent> {
        let _span = tracy_client::span!("AssetHub::update");
        let mut events = Vec::new();

        while let Some(result) = self.loader.try_recv_result() {
            match result {
                LoadResult::Success { handle, data } => {
                    if let Some(record) = self.textures.get_mut(handle) {
                        record.status = LoadStatus::Ready;
                    }

                    events.push(LoadedAssetEvent::TextureLoaded { handle, data });
                }
                LoadResult::Failure(handle, error) => {
                    if let Some(record) = self.textures.get_mut(handle) {
                        record.status = LoadStatus::Failed;
                    }

                    events.push(LoadedAssetEvent::TextureFailed { handle, error });
                }
            }
        }

        events
    }
}
