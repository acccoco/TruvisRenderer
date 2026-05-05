//! Base frame orchestration runtime.
//!
//! [`BaseApp`] drives the invariant per-frame lifecycle:
//! `begin_frame` → input hook → update hook → prepare → render hook → present → `end_frame`.
//!
//! Concrete apps own GUI, camera, input state, overlays, and render plugins.

mod base_app;

pub use base_app::{BaseApp, init_env};
