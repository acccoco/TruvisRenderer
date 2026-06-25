//! 内容资产的 CPU 侧身份、加载状态与完成事件系统。
//!
//! 本 crate 位于 World 层和 RenderRuntime 之间：[`AssetHub`](asset_hub::AssetHub)
//! 持有 texture / mesh / material / model 的内容 handle、去重表和 CPU 加载状态；
//! texture bytes 与 mesh 数据通过加载事件一次性交给 `AssetTextureManager`、
//! `AssetMeshManager`、`MaterialBridge` 继续创建 GPU image、buffer、BLAS 与
//! bindless / material slot。material 和 model asset 的 CPU 数据仍保存在 asset 层，
//! model asset 只保存可重复 spawn 的 prefab 数据，运行时 instance 由
//! `truvis-world` 的 `SceneManager` 创建和管理。
//!
//! 这里所有 `Ready` 状态都只表示 CPU 数据已经可读取，不表示 GPU 资源或 shader
//! 可见绑定已经完成。
//!
//! # 加载 Pipeline
//!
//! ```text
//! load_texture(path) / load_model(path) / register_mesh_data(key, data)
//!       │
//!       ▼
//!   ┌──────────────┐   rayon / importer   ┌────────────────────────┐
//!   │ CPU Loading  │ ───────────────────▶ │ upload-ready CPU data  │
//!   │ / Ready      │                      │ texture/mesh/model     │
//!   └──────────────┘                      └────────────────────────┘
//!          │
//!          ▼
//!   AssetLoadedEvent -> render backend manager
//! ```
//!
//! - [`AssetHub`](asset_hub::AssetHub) — 统一入口、内容去重、CPU 状态管理和上传事件汇聚
//! - [`LoadStatus`](handle::LoadStatus) — CPU 侧资源状态机（Loading → Ready / Failed）
//! - 内部 loader 模块 — 后台调度、纹理解码与 Assimp / glTF model 导入，不作为 crate 对外 API

pub mod asset_hub;
pub mod handle;

pub(crate) mod asset_loader;
pub(crate) mod gltf_scene_loader;
pub(crate) mod texture_loader;
pub(crate) mod truvixx_scene_loader;
