//! Render thread main loop.
//!
//! Drives [`FrameRuntime`] through its public API only.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use truvis_app_api::app_plugin::AppPlugin;
use truvis_frame_runtime::FrameRuntime;

use crate::shared::{RenderInitMsg, SharedState, unpack_size};

/// Render thread entry point.
pub fn render_loop(shared: Arc<SharedState>, init_msg: RenderInitMsg, plugin: Box<dyn AppPlugin>) {
    tracy_client::set_thread_name!("RenderThread");

    let raw_display = init_msg.raw_display.0;
    let raw_window = init_msg.raw_window.0;

    let mut runtime = FrameRuntime::new_with_plugin(raw_display, plugin);
    runtime.init_after_window(raw_display, raw_window, init_msg.scale_factor, init_msg.initial_size);

    let mut last_built_size = init_msg.initial_size;

    while !shared.exit.load(Ordering::Acquire) {
        while let Ok(event) = shared.event_receiver.try_recv() {
            runtime.push_input_event(event);
        }

        let [w, h] = unpack_size(shared.size.load(Ordering::Relaxed));
        if w == 0 || h == 0 {
            std::thread::park_timeout(Duration::from_millis(1));
            continue;
        }

        runtime.recreate_swapchain_if_needed([w, h], &mut last_built_size);

        if !runtime.time_to_render() {
            std::thread::park_timeout(Duration::from_millis(1));
            continue;
        }

        runtime.run_frame();
    }

    log::info!("RenderThread: exit flag observed, destroying resources.");
    runtime.destroy();
}
