//! 帧编排运行时模块的语义入口。
//!
//! 新代码应通过 `truvis_app::frame_runtime::FrameRuntime` 引入。
//! 旧路径 `truvis_app::render_app::RenderApp` 在兼容窗口内仍可用。

pub use crate::render_app::FrameRuntime;
