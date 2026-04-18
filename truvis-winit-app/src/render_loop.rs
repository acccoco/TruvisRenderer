//! 渲染线程主循环。
//!
//! 渲染线程在独立 OS 线程中运行，负责：
//! 1. 接收主线程投递的 [`RenderInitMsg`]，执行 Vulkan 初始化；
//! 2. 每轮循环 drain 共享事件通道、检测 `size` atomic 决定是否重建 swapchain；
//! 3. 通过 `Renderer::time_to_render()` 自主决定推进帧时机；
//! 4. 观察到 `shared.exit` 后销毁所有 Vulkan 资源并置位 `render_finished`。

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use truvis_app::app_plugin::AppPlugin;
use truvis_app::frame_runtime::FrameRuntime;

use crate::shared::{RenderInitMsg, SharedState, unpack_size};

/// 渲染线程入口（新契约路径）。直接接受 [`AppPlugin`]。
///
/// 返回时意味着已完成 `Gfx::wait_idle` 与资源销毁；调用方（`WinitApp` 的
/// 线程 wrapper）会在此之后置位 `render_finished`。
pub fn render_loop(shared: Arc<SharedState>, init_msg: RenderInitMsg, plugin: Box<dyn AppPlugin>) {
    tracy_client::set_thread_name!("RenderThread");

    let raw_display = init_msg.raw_display.0;
    let raw_window = init_msg.raw_window.0;
    let mut render_app = FrameRuntime::new_with_plugin(raw_display, plugin);
    render_app.init_after_window(raw_display, raw_window, init_msg.scale_factor, init_msg.initial_size);

    let mut last_built_size = init_msg.initial_size;

    while !shared.exit.load(Ordering::Acquire) {
        while let Ok(event) = shared.event_receiver.try_recv() {
            render_app.input_manager.push_event(event);
        }

        let [w, h] = unpack_size(shared.size.load(Ordering::Relaxed));
        if w == 0 || h == 0 {
            std::thread::park_timeout(Duration::from_millis(1));
            continue;
        }

        render_app.recreate_swapchain_if_needed([w, h], &mut last_built_size);

        if !render_app.renderer.time_to_render() {
            std::thread::park_timeout(Duration::from_millis(1));
            continue;
        }

        render_app.big_update();
    }

    log::info!("RenderThread: exit flag observed, destroying resources.");
    render_app.destroy();
}
