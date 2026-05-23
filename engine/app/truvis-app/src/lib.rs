//! 应用框架层。
//!
//! 提供 demo app、render pass 和 GUI plugin 集成。
//!
//! **新代码** 应直接从 [`truvis_app_frame`] 导入 App 框架契约与帧骨架。这个 crate 提供具体 app state，
//! 由 app state 持有 GUI、camera/input state、overlay 和 render pipeline plugin。
//! `RenderAppShell` 持有 RenderRuntime，并将该 state 适配到 render-loop `RenderApp` 契约。

pub mod camera_controller;
pub mod gui_plugin;
pub mod input_state;
pub mod outer_app;
pub mod overlay;
pub mod render_pipeline;
