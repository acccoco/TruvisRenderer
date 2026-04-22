//! Application framework layer.
//!
//! Re-exports contracts from [`truvis_app_api`] and runtime from [`truvis_frame_runtime`],
//! plus demo apps, render passes, and the GUI render-graph integration.
//!
//! **New code** should import from [`truvis_app_api`] (contracts) or
//! [`truvis_frame_runtime`] (runtime) directly. This crate provides
//! transition-period re-exports so existing import paths continue to work.

pub mod gui_rg_pass;
pub mod outer_app;
pub mod render_pipeline;
