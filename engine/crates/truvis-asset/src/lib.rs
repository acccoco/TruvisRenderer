//! 异步资产加载系统
//!
//! 将磁盘 IO 和 CPU 解码放到后台执行，避免启动阻塞和运行时卡顿。
//! GPU 上传、image/view 创建和 bindless 注册由渲染后端负责。
//!
//! # 加载 Pipeline
//!
//! ```text
//! load_texture(path)
//!       │
//!       ▼
//!   ┌────────┐   rayon    ┌──────────────┐
//!   │Loading │ ─────────▶ │ Loaded bytes │
//!   │(IO+解码)│           │(CPU RGBA8)   │
//!   └────────┘            └──────────────┘
//! ```
//!
//! - [`AssetHub`](asset_hub::AssetHub) — 统一接口、路径去重、状态管理
//! - [`AssetLoader`](asset_loader::AssetLoader) — 后台文件读取与 CPU 解码
//! - [`LoadStatus`](handle::LoadStatus) — 资源状态机（Loading → Ready / Failed）

pub mod asset_hub;
pub mod asset_loader;
pub mod handle;
