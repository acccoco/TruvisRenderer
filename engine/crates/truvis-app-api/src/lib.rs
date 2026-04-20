//! Plugin contract and phase context definitions.
//!
//! This crate defines the stable API surface for application plugins:
//! - [`AppPlugin`](app_plugin::AppPlugin) — typed-context plugin trait
//! - Phase contexts: [`InitCtx`], [`UpdateCtx`], [`RenderCtx`], [`ResizeCtx`]
//! - [`OverlayModule`](overlay::OverlayModule) — registrable UI overlay trait
//! - [`InputEvent`](input_event::InputEvent) — platform input event types

pub mod app_plugin;
pub mod input_event;
pub mod overlay;
