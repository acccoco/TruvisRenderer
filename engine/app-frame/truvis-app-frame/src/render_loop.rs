//! 渲染线程主循环。
//!
//! 只通过公开 API 驱动 [`RenderApp`]。

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::render_app_api::RenderApp;
use crate::render_thread::{RenderInitMsg, SharedState, unpack_size};

/// 渲染线程入口。
pub fn render_loop(shared: Arc<SharedState>, init_msg: RenderInitMsg, mut app: Box<dyn RenderApp>) {
    tracy_client::set_thread_name!("RenderThread");

    let raw_display = init_msg.raw_display.0;
    let raw_window = init_msg.raw_window.0;

    app.init_after_window(raw_display, raw_window, init_msg.scale_factor, init_msg.initial_size);

    let mut last_built_size = init_msg.initial_size;
    let mut last_seen_resize_generation = shared.resize_generation.load(Ordering::Acquire);
    let mut pending_resize_since: Option<Instant> = None;
    const RESIZE_DEBOUNCE: Duration = Duration::from_millis(80);

    while !shared.exit.load(Ordering::Acquire) {
        while let Ok(event) = shared.event_receiver.try_recv() {
            app.push_input_event(event);
        }

        let resize_generation = shared.resize_generation.load(Ordering::Acquire);
        if resize_generation != last_seen_resize_generation {
            last_seen_resize_generation = resize_generation;
            pending_resize_since = Some(Instant::now());
        }

        let [w, h] = unpack_size(shared.size.load(Ordering::Relaxed));
        if w == 0 || h == 0 {
            std::thread::park_timeout(Duration::from_millis(1));
            continue;
        }

        if [w, h] != last_built_size && pending_resize_since.is_none() {
            pending_resize_since = Some(Instant::now());
        }

        if let Some(resize_since) = pending_resize_since {
            if resize_since.elapsed() < RESIZE_DEBOUNCE {
                std::thread::park_timeout(Duration::from_millis(1));
                continue;
            }

            app.recreate_swapchain_if_needed([w, h]);
            last_built_size = [w, h];
            pending_resize_since = None;
        }

        if app.has_pending_swapchain_recreate() {
            app.recreate_swapchain_if_needed([w, h]);
            last_built_size = [w, h];
        }

        if !app.time_to_render() {
            std::thread::park_timeout(Duration::from_millis(1));
            continue;
        }

        app.run_frame();
    }

    log::info!("RenderThread: exit flag observed, destroying resources.");
    app.shutdown();
}
