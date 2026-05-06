//! 渲染线程主循环。
//!
//! 只通过公开 API 驱动 [`FrameApp`]。

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use truvis_frame_api::frame_app::FrameApp;

use crate::shared::{RenderInitMsg, SharedState, unpack_size};

/// 渲染线程入口。
pub fn render_loop(shared: Arc<SharedState>, init_msg: RenderInitMsg, mut app: Box<dyn FrameApp>) {
    tracy_client::set_thread_name!("RenderThread");

    let raw_display = init_msg.raw_display.0;
    let raw_window = init_msg.raw_window.0;

    app.init_after_window(raw_display, raw_window, init_msg.scale_factor, init_msg.initial_size);

    let mut last_built_size = init_msg.initial_size;

    while !shared.exit.load(Ordering::Acquire) {
        while let Ok(event) = shared.event_receiver.try_recv() {
            app.push_input_event(event);
        }

        let [w, h] = unpack_size(shared.size.load(Ordering::Relaxed));
        if w == 0 || h == 0 {
            std::thread::park_timeout(Duration::from_millis(1));
            continue;
        }

        if [w, h] != last_built_size {
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
