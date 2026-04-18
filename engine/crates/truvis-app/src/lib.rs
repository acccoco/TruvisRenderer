//! 应用框架层
//!
//! 提供基于 [`AppPlugin`](app_plugin::AppPlugin) trait 的应用开发模式，集成窗口系统、输入处理、GUI 等功能。
//! 新代码应实现 `AppPlugin` trait 并通过 [`FrameRuntime`](frame_runtime::FrameRuntime) 接入帧编排。
//!
//! 旧 [`OuterApp`](outer_app::base::OuterApp) 路径在兼容窗口内通过
//! [`LegacyOuterAppAdapter`](app_plugin::LegacyOuterAppAdapter) 继续可用。

pub mod app_plugin;
pub mod frame_runtime;
pub mod gui_front;
pub mod gui_rg_pass;
pub mod outer_app;
pub mod overlay;
pub mod platform;
pub mod render_app;
pub mod render_pipeline;
