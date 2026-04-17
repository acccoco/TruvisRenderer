//! 异步资源加载系统
//!
//! 将磁盘 IO、CPU 解码、GPU 上传全流程后台执行，避免启动阻塞和运行时卡顿。
//!
//! # 加载 Pipeline
//!
//! ```text
//! load_texture(path)
//!       │
//!       ▼
//!   ┌────────┐   rayon    ┌────────────┐  Transfer Queue  ┌───────┐
//!   │Loading │ ─────────▶ │ Uploading  │ ────────────────▶ │ Ready │
//!   │(IO+解码)│           │(Staging+Copy)│  Timeline Sem   │(GPU可用)│
//!   └────────┘            └────────────┘                  └───────┘
//! ```
//!
//! - [`AssetHub`](asset_hub::AssetHub) — 统一接口、状态管理、Fallback（未就绪时返回 1×1 粉色纹理）
//! - [`AssetLoader`](asset_loader::AssetLoader) — 后台 IO 线程，文件读取与 CPU 解码
//! - [`AssetUploadManager`](asset_upload_manager::AssetUploadManager) — Staging Buffer 管理、Transfer Queue 提交、Timeline Semaphore 同步
//! - [`LoadStatus`](handle::LoadStatus) — 资源状态机（Loading → Uploading → Ready / Failed）

pub mod asset_hub;
pub mod asset_loader;
pub mod asset_upload_manager;
pub mod handle;
