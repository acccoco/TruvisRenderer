//! 应用框架层。
//!
//! 重新导出 [`truvis_frame_api`] 的契约与 [`truvis_frame_runtime`] 的运行时，
//! 并提供 demo app、render pass 和 GUI plugin 集成。
//!
//! **新代码** 应直接从 [`truvis_frame_api`]（契约）或
//! [`truvis_frame_runtime`]（运行时）导入。这个 crate 提供具体 app state，
//! 由 app state 持有 GUI、camera/input state、overlay 和 render pipeline plugin。
//! `FrameAppShell` 持有 `BaseApp`，并将该 state 适配到 render-loop `FrameApp` 契约。

pub mod camera_controller;
pub mod gui_plugin;
pub mod input_state;
pub mod outer_app;
pub mod overlay;
pub mod render_pipeline;
