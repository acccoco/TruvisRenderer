//! 渲染后端层
//!
//! 提供被 `RenderAppShell` 驱动的 GPU/backend 执行能力，包括 swapchain、present、
//! command/sync 生命周期、GPU 场景上传和描述符更新。

pub mod platform;
pub mod present;
pub mod subsystems;

pub mod asset_mesh_uploader;
pub mod asset_texture_uploader;
mod instance_bridge;
mod material_bridge;
pub mod model_loader;
pub mod render_backend;
