//! 一次性 CPU asset loader 与完成事件系统。
//!
//! 本 crate 位于 World 层和 RenderRuntime 之间：[`AssetHub`](asset_hub::AssetHub)
//! 只负责提交 texture/model CPU load task，并把完成结果作为一次性事件交给
//! `truvis-world` 的 `SceneAssetIngestor`。长期 scene identity、texture 去重、
//! model ingest transaction 和 render upload payload 都不属于 asset 层。
//!
//! 这里所有 `Ready` 状态都只表示 CPU 数据已经可读取，不表示 GPU 资源或 shader
//! 可见绑定已经完成。
//!
//! # 加载 Pipeline
//!
//! ```text
//! request_texture(TextureLoadDesc) / request_model(ModelLoadDesc)
//!       │
//!       ▼
//!   ┌──────────────┐   rayon / importer   ┌────────────────────────┐
//!   │ CPU Loading  │ ───────────────────▶ │ upload-ready CPU data  │
//!   │ / Ready      │                      │ texture/mesh/model     │
//!   └──────────────┘                      └────────────────────────┘
//!          │
//!          ▼
//!   AssetLoadEvent -> SceneAssetIngestor -> render backend manager
//! ```
//!
//! - [`AssetHub`](asset_hub::AssetHub) — 一次性 loader task 入口和完成事件汇聚
//! - [`LoadStatus`](handle::LoadStatus) — CPU 侧资源状态机（Loading → Ready / Failed）
//! - 内部 loader 模块 — 后台调度、纹理解码与 Assimp / glTF model 导入，不作为 crate 对外 API

pub mod asset_hub;
pub mod handle;

pub(crate) mod asset_loader;
pub(crate) mod gltf_scene_loader;
pub(crate) mod texture_loader;
pub(crate) mod truvixx_scene_loader;
