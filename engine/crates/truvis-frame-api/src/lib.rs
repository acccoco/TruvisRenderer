//! Plugin contract and phase context definitions.
//!
//! This crate defines the stable API surface for frame plugins:
//! - [`FramePlugin`](frame_plugin::FramePlugin) — typed-context plugin trait
//! - Phase contexts: [`InitCtx`], [`UpdateCtx`], [`RenderCtx`], [`ResizeCtx`]
//! - [`OverlayModule`](overlay::OverlayModule) — registrable UI overlay trait
//! - [`InputEvent`](input_event::InputEvent) — platform input event types

pub mod frame_plugin;
pub mod input_event;
pub mod overlay;
