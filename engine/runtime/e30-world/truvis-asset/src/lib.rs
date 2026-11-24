//! 共享资产身份、CPU payload 和一次性加载服务。
//!
//! [`AssetLoadService`](asset_load_service::AssetLoadService) 交付 owned 内存结果。
//! Runtime 的 AssetImports 在 update 边界把结果应用到 AssetSystem。
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
//!   AssetLoadEvent -> AssetImports -> AssetSystem
//! ```
//!
//! - [`AssetLoadService`](asset_load_service::AssetLoadService) — 一次性 loader task 入口和完成事件汇聚
//! - [`LoadStatus`](handle::LoadStatus) — CPU 侧资源状态机（Loading → Ready / Failed）
//! - 内部 loader 模块 — 后台调度、纹理解码与 Assimp / glTF scene 导入，不作为 crate 对外 API

pub mod asset_load_service;
pub mod handle;
pub mod material;
pub mod material_texture;

pub(crate) mod asset_load_worker;
pub(crate) mod gltf_scene_loader;
pub(crate) mod texture_loader;
pub(crate) mod truvixx_scene_loader;
