//! Application framework layer.
//!
//! Re-exports contracts from [`truvis_app_api`] and runtime from [`truvis_frame_runtime`],
//! plus demo apps, render passes, and the GUI render-graph integration.
//!
//! **New code** should import from [`truvis_app_api`] (contracts) or
//! [`truvis_frame_runtime`] (runtime) directly. This crate provides
//! transition-period re-exports so existing import paths continue to work.

pub mod app_plugin;
pub mod frame_runtime;
pub mod gui_front;
pub mod gui_rg_pass;
pub mod outer_app;
pub mod overlay;
pub mod platform;
pub mod render_app;
pub mod render_pipeline;
