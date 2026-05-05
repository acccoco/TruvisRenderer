//! Frame app and plugin contracts shared by runtime and app crates.
//!
//! This crate defines only contracts and typed contexts:
//! - [`FrameApp`](frame_app::FrameApp) — object-safe render-loop app contract
//! - [`FrameAppHooks`](frame_app::FrameAppHooks) — hook points used by `BaseApp`
//! - [`Plugin`](plugin::Plugin) — reusable capability lifecycle contract
//! - [`InputEvent`](input_event::InputEvent) — platform input event types

pub mod frame_app;
pub mod input_event;
pub mod plugin;
