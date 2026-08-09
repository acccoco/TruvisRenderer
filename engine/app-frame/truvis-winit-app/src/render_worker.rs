use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::{self, JoinHandle};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

use truvis_app_frame::render_app_api::RenderApp;
use truvis_app_frame::{RenderAppShell, RenderInitMsg, SendWrapper, SharedState, render_loop};

pub(crate) type RenderAppFactory = Box<dyn FnOnce() -> Box<dyn RenderApp> + Send + 'static>;

/// 平台窗口与 `RenderThread` 之间的共同生命周期 owner。
///
/// standalone 与 embedded 两种窗口模式都通过本类型创建相同的 [`SharedState`]、
/// raw-handle 初始化消息和渲染线程。窗口 owner 必须在 [`Self::finish`] 返回前保持
/// `Window` 存活，从而保证 Vulkan surface 的销毁严格早于 HWND 销毁。
pub(crate) struct RenderWorker {
    shared: Arc<SharedState>,
    join_handle: Option<JoinHandle<()>>,
}

impl RenderWorker {
    pub(crate) fn spawn(
        window: &Window,
        app_factory: RenderAppFactory,
        on_finished: impl FnOnce() + Send + 'static,
    ) -> Result<Self, String> {
        let window_size = window.inner_size();
        let initial_size = [window_size.width, window_size.height];
        let shared = Arc::new(SharedState::new(initial_size));
        let init_msg = RenderInitMsg {
            raw_display: SendWrapper(
                window.display_handle().map_err(|error| format!("failed to get display handle: {error}"))?.as_raw(),
            ),
            raw_window: SendWrapper(
                window.window_handle().map_err(|error| format!("failed to get window handle: {error}"))?.as_raw(),
            ),
            scale_factor: window.scale_factor(),
            initial_size,
        };
        let shared_for_thread = shared.clone();

        let join_handle = thread::Builder::new()
            .name("RenderThread".to_string())
            .spawn(move || {
                let shared_in_thread = shared_for_thread;
                let result = panic::catch_unwind(AssertUnwindSafe(|| {
                    let app = app_factory();
                    let shell = RenderAppShell::new(app);
                    render_loop(shared_in_thread.clone(), init_msg, shell);
                }));
                if let Err(payload) = result {
                    log::error!("RenderThread panicked; capturing payload for the window owner.");
                    if let Ok(mut slot) = shared_in_thread.panic_payload.lock() {
                        *slot = Some(payload);
                    }
                }
                shared_in_thread.exit.store(true, Ordering::Release);
                shared_in_thread.render_finished.store(true, Ordering::Release);
                on_finished();
            })
            .map_err(|error| format!("failed to spawn RenderThread: {error}"))?;

        Ok(Self {
            shared,
            join_handle: Some(join_handle),
        })
    }

    pub(crate) fn shared(&self) -> &Arc<SharedState> {
        &self.shared
    }

    pub(crate) fn request_exit(&self) {
        self.shared.exit.store(true, Ordering::Release);
    }

    /// 等待 Vulkan owner 完整退出，并把渲染线程捕获的 panic 交给窗口 owner 处理。
    pub(crate) fn finish(mut self) -> Option<Box<dyn Any + Send>> {
        self.request_exit();
        if let Some(handle) = self.join_handle.take() {
            if let Err(error) = handle.join() {
                log::error!("RenderThread join returned Err: {error:?}");
            }
        }
        self.shared.panic_payload.lock().ok().and_then(|mut payload| payload.take())
    }
}
