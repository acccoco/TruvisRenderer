//! Frame orchestration runtime.
//!
//! [`FrameRuntime`] drives the per-frame lifecycle:
//!   `begin_frame` → `phase_input` → `phase_update` → `phase_prepare` → `phase_render` → `phase_present`
//!
//! External callers interact only through the public runtime API:
//! [`push_input_event`](FrameRuntime::push_input_event),
//! [`time_to_render`](FrameRuntime::time_to_render),
//! [`recreate_swapchain_if_needed`](FrameRuntime::recreate_swapchain_if_needed),
//! [`run_frame`](FrameRuntime::run_frame),
//! [`destroy`](FrameRuntime::destroy).
//!
//! `phase_prepare` is owned exclusively by the runtime — not exposed as an `AppPlugin` hook.

mod camera_controller;
mod frame_runtime;
mod gui_front;
mod input_manager;
mod input_state;

pub use frame_runtime::FrameRuntime;
