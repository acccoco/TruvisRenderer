//! Truvis Web 编辑器的 loopback HTTP / WebSocket Server。
//!
//! Server 运行在独立 OS 线程的 Tokio current-thread runtime 中，只负责静态文件、
//! WebSocket、JSON 和 client 路由。它不依赖 `truvis-world` 或任何 GPU crate。

mod config;
mod handle;
mod runtime;
mod server;

pub use config::EditorServerConfig;
pub use handle::EditorServerHandle;
pub use server::EditorServer;
