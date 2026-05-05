//! Application framework layer.
//!
//! Re-exports contracts from [`truvis_frame_api`] and runtime from [`truvis_frame_runtime`],
//! plus demo apps, render passes, and the GUI plugin integration.
//!
//! **New code** should import from [`truvis_frame_api`] (contracts) or
//! [`truvis_frame_runtime`] (runtime) directly. This crate provides
//! Concrete apps own `BaseApp`, GUI, camera/input state, overlays, and render
//! pipeline plugins.

pub mod camera_controller;
pub mod gui_plugin;
pub mod input_state;
pub mod outer_app;
pub mod overlay;
pub mod render_pipeline;
