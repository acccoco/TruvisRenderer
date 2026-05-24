//! App 层公共组件。
//!
//! 本 crate 只保存可被主体 app 和 samples 复用的 app 集成能力，例如 GUI、
//! 输入/相机控制、overlay 与 RT pipeline。具体 app state 与 sample 专用 pass
//! 不放在这里，避免公共层反向承担业务入口职责。

pub mod camera;
pub mod camera_controller;
pub mod gui_plugin;
pub mod input_state;
pub mod overlay;
pub mod render_pipeline;
