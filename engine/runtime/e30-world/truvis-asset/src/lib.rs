//! 一次性 CPU asset loader 与完成事件系统。
//!
//! 本 crate 位于 GameWorld 层和 RenderRuntime 之间：[`AssetLoadService`](asset_load_service::AssetLoadService)
//! 只负责提交 texture/scene CPU load task，并把完成结果作为一次性事件交给
//! `truvis-world` 的 `AssetSystem`。长期 scene identity、texture 去重、
//! scene ingest 协调和 render upload 都不属于 asset 层。
//!
//! 这里所有 `Ready` 状态都只表示 CPU 数据已经可读取，不表示 GPU 资源或 shader
//! 可见绑定已经完成。
//!
//! # 加载 Pipeline
//!
//! ```text
//! request_texture(TextureLoadDesc) / request_scene(SceneLoadDesc)
//!       │
//!       ▼
//!   ┌──────────────┐   rayon / importer   ┌────────────────────────┐
//!   │ CPU Loading  │ ───────────────────▶ │ upload-ready CPU data  │
//!   │ / Ready      │                      │ texture/mesh/scene     │
//!   └──────────────┘                      └────────────────────────┘
//!          │
//!          ▼
//!   AssetLoadEvent -> AssetSystem -> render backend manager
//! ```
//!
//! - [`AssetLoadService`](asset_load_service::AssetLoadService) — 一次性 loader task 入口和完成事件汇聚
//! - [`LoadStatus`](handle::LoadStatus) — CPU 侧资源状态机（Loading → Ready / Failed）
//! - 内部 loader 模块 — 后台调度、纹理解码与 Assimp / glTF scene 导入，不作为 crate 对外 API

pub mod asset_load_service;
pub mod handle;
pub mod material_texture;

pub(crate) mod asset_load_worker;
pub(crate) mod gltf_scene_loader;
pub(crate) mod texture_loader;
pub(crate) mod truvixx_scene_loader;
