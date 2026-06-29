//! `World` 到 `RenderWorld` 的 prepare 同步包。
//!
//! 本模块只定义 CPU scene / asset ingest 输出给渲染侧的短期 payload。这里不保存
//! Vulkan 资源、GPU slot 或跨帧状态；payload 在 `RenderWorld::prepare_asset_sync` 中被消费后
//! 即可释放。

use truvis_asset::handle::{MeshData, TextureBytes};

use crate::guid_new_type::{SceneMeshHandle, SceneTextureHandle};
use crate::scene_store::SceneChanges;

/// `World::sync_for_render` 交给 `RenderWorld` 的本帧同步包。
///
/// 这里同时携带 CPU 语义 change log 与 asset upload payload，避免 begin-frame、prepare
/// 两条路径分别消费 CPU scene 状态。
#[derive(Debug, Default)]
pub struct WorldRenderSync {
    pub scene_changes: SceneChanges,
    pub asset_uploads: SceneAssetSyncOutput,
}

/// `SceneAssetIngestor` 输出给 render-side manager 的分组 asset payload。
///
/// 它已经完成 `TextureLoadHandle` / `ModelLoadHandle` 到 `Scene*Handle` 的翻译。各
/// render manager 只消费自己需要的窄 payload，不再接收宽 enum 后用 `unreachable!`
/// 防御错误事件类型。
#[derive(Debug, Default)]
pub struct SceneAssetSyncOutput {
    pub pending_texture_uploads: Vec<PendingTextureUpload>,
    pub failed_textures: Vec<FailedTextureLoad>,
    pub pending_mesh_uploads: Vec<PendingMeshUpload>,
}

/// 已完成 CPU decode、等待 render-side texture manager 提交 GPU upload 的短期 payload。
#[derive(Debug)]
pub struct PendingTextureUpload {
    pub handle: SceneTextureHandle,
    pub data: TextureBytes,
}

/// texture CPU load 失败事件。
///
/// 失败仍按 scene texture handle 汇报；render-side texture resolver 继续提供 fallback binding。
#[derive(Debug)]
pub struct FailedTextureLoad {
    pub handle: SceneTextureHandle,
    pub error: String,
}

/// 已完成 CPU import、等待 render-side mesh manager 提交 vertex/index/BLAS upload 的短期 payload。
#[derive(Debug)]
pub struct PendingMeshUpload {
    pub handle: SceneMeshHandle,
    pub data: MeshData,
}
