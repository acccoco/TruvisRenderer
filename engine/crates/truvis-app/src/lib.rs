//! 应用框架层
//!
//! 提供基于 [`OuterApp`] trait 的应用开发模式，集成窗口系统、输入处理、GUI 等功能。
//! 开发者只需实现 [`OuterApp`] trait，即可快速构建渲染应用。

pub mod gui_front;
pub mod gui_rg_pass;
pub mod outer_app;
pub mod platform;
pub mod render_app;
pub mod render_pipeline;
