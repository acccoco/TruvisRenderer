//! 资产 CPU 数据与加载事件系统
//!
//! 将纹理磁盘 IO 和 CPU 解码放到后台执行，同时接收导入器复制出来的 owned mesh CPU 数据。
//! GPU 上传、image/view 创建、vertex/index buffer、BLAS 和 bindless 注册由渲染后端负责。
//!
//! # 加载 Pipeline
//!
//! ```text
//! load_texture(path) / register_mesh_data(key, data)
//!       │
//!       ▼
//!   ┌────────┐   rayon / importer   ┌──────────────┐
//!   │Loading │ ───────────────────▶ │ CPU data     │
//!   │/ Ready │                      │ texture/mesh │
//!   └────────┘                      └──────────────┘
//! ```
//!
//! - [`AssetHub`](asset_hub::AssetHub) — 统一接口、路径去重、状态管理
//! - [`AssetLoader`](asset_loader::AssetLoader) — 后台文件读取与 CPU 解码
//! - [`LoadStatus`](handle::LoadStatus) — CPU 侧资源状态机（Loading → Ready / Failed）

pub mod asset_hub;
pub mod asset_loader;
pub mod handle;
