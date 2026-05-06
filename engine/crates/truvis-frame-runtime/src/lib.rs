//! 基础帧编排运行时。
//!
//! [`BaseApp`] 驱动固定的每帧生命周期：
//! `begin_frame` → 输入 hook → update hook → prepare → render hook → present → `end_frame`。
//!
//! [`FrameAppShell`] 持有 `BaseApp` 并驱动具体 app state。具体 app state
//! 持有 GUI、camera、input state、overlay 和 render plugin。

mod base_app;
mod frame_app_shell;

pub use base_app::{BaseApp, init_env};
pub use frame_app_shell::{FrameAppInitCtx, FrameAppResizeCtx, FrameAppShell, FrameAppState};
