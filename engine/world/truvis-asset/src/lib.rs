//! 内容资产的 CPU 侧身份、加载状态与完成事件系统。
//!
//! 本 crate 位于 World 层和 RenderBackend 之间：[`AssetHub`](asset_hub::AssetHub)
//! 持有 texture / mesh / material / scene 的内容 handle、去重表和 CPU ready 数据；
//! 渲染后端通过加载事件把这些数据交给 `AssetTextureUploader`、
//! `AssetMeshUploader`、`MaterialBridge` 继续创建 GPU image、buffer、BLAS 与
//! bindless / material slot。scene asset 只保存可重复 spawn 的 prefab CPU 数据，
//! 运行时 instance 由 `truvis-scene` 的 `SceneManager` 创建和管理。
//!
//! 这里所有 `Ready` 状态都只表示 CPU 数据已经可读取，不表示 GPU 资源或 shader
//! 可见绑定已经完成。
//!
//! # 加载 Pipeline
//!
//! ```text
//! load_texture(path) / load_scene(path) / register_mesh_data(key, data)
//!       │
//!       ▼
//!   ┌──────────────┐   rayon / importer   ┌────────────────────────┐
//!   │ CPU Loading  │ ───────────────────▶ │ upload-ready CPU data  │
//!   │ / Ready      │                      │ texture/mesh/scene     │
//!   └──────────────┘                      └────────────────────────┘
//!          │
//!          ▼
//!   LoadedAssetEvent -> render backend uploader / SceneManager
//! ```
//!
//! - [`AssetHub`](asset_hub::AssetHub) — 统一入口、内容去重、CPU 状态管理和完成事件汇聚
//! - [`LoadStatus`](handle::LoadStatus) — CPU 侧资源状态机（Loading → Ready / Failed）
//! - 内部 loader 模块 — 后台调度、纹理解码与 Assimp scene 导入，不作为 crate 对外 API

pub mod asset_hub;
pub mod handle;

pub(crate) mod asset_loader;
pub(crate) mod texture_loader;
pub(crate) mod truvixx_scene_loader;
