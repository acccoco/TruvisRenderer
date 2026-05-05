//! Base frame orchestration runtime.
//!
//! [`BaseApp`] drives the invariant per-frame lifecycle:
//! `begin_frame` → input hook → update hook → prepare → render hook → present → `end_frame`.
//!
//! [`FrameAppShell`] owns `BaseApp` and drives concrete app state. Concrete app
//! state owns GUI, camera, input state, overlays, and render plugins.

mod base_app;
mod frame_app_shell;

pub use base_app::{BaseApp, init_env};
pub use frame_app_shell::{FrameAppInitCtx, FrameAppResizeCtx, FrameAppShell, FrameAppState};
