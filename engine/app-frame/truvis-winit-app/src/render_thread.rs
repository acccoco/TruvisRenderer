use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

use truvis_app_frame::input_event::InputEvent;
use truvis_app_frame::render_app_api::RenderApp;
use truvis_app_frame::{RenderAppRunner, RenderThreadControl, RenderThreadInit};

pub(crate) type RenderAppFactory = Box<dyn FnOnce() -> Box<dyn RenderApp> + Send + 'static>;

/// 跨线程传递 `!Send` 平台窗口 handle 的受控包装。
///
/// # Safety
/// 调用方必须保证 handle 指向的窗口始终活到目标线程完成 surface 销毁；目标线程
/// 只能将其用于预期的平台窗口创建或 Vulkan surface 初始化，不得解引用或再次分发。
pub struct SendWrapper<T>(pub T);

impl<T> SendWrapper<T> {
    /// 通过方法消费整个 wrapper，避免 Rust 精确捕获把内部 `!Send` handle 直接移入 closure。
    pub(crate) fn into_inner(self) -> T {
        self.0
    }
}

// Safety: 平台窗口 owner 通过 RenderThread::join 和 EmbeddedWinitHost::shutdown 保证生命周期顺序。
unsafe impl<T> Send for SendWrapper<T> {}

/// standalone / embedded 窗口 owner 共同持有的 OS RenderThread 生命周期句柄。
///
/// 本类型收拢 thread spawn、输入/resize 发布、退出握手和 join；App factory 只在
/// OS RenderThread 内执行，随后交给唯一的 [`RenderAppRunner`] 驱动完整生命周期。
/// 窗口 owner 必须在 [`Self::join`] 返回前保持 `Window` 存活。
pub(crate) struct RenderThread {
    control: Arc<RenderThreadControl>,
    join_handle: Option<JoinHandle<()>>,
}

impl RenderThread {
    pub(crate) fn spawn(
        window: &Window,
        app_factory: RenderAppFactory,
        on_finished: impl FnOnce() + Send + 'static,
    ) -> Result<Self, String> {
        let window_size = window.inner_size();
        let initial_size = [window_size.width, window_size.height];
        let control = Arc::new(RenderThreadControl::new(initial_size));
        let init = SendWrapper(RenderThreadInit {
            raw_display: window.display_handle().map_err(|error| format!("failed to get display handle: {error}"))?.as_raw(),
            raw_window: window.window_handle().map_err(|error| format!("failed to get window handle: {error}"))?.as_raw(),
            scale_factor: window.scale_factor(),
            initial_size,
        });
        let control_for_thread = Arc::clone(&control);

        let join_handle = thread::Builder::new()
            .name("RenderThread".to_string())
            .spawn(move || {
                let control_in_thread = control_for_thread;
                let result = panic::catch_unwind(AssertUnwindSafe(|| {
                    let init = init.into_inner();
                    let app = app_factory();
                    RenderAppRunner::run(Arc::clone(&control_in_thread), init, app);
                }));
                if let Err(payload) = result {
                    log::error!("RenderThread panicked; capturing payload for the window owner.");
                    control_in_thread.record_panic(payload);
                }
                control_in_thread.request_exit();
                control_in_thread.mark_finished();
                on_finished();
            })
            .map_err(|error| format!("failed to spawn RenderThread: {error}"))?;

        Ok(Self {
            control,
            join_handle: Some(join_handle),
        })
    }

    pub(crate) fn request_exit(&self) {
        self.control.request_exit();
    }

    pub(crate) fn publish_resize(&self, size: [u32; 2]) {
        self.control.publish_resize(size);
    }

    pub(crate) fn send_input(&self, event: InputEvent) {
        self.control.send_input(event);
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.control.is_finished()
    }

    /// 等待 Vulkan owner 完整退出，并把渲染线程捕获的 panic 交给窗口 owner 处理。
    pub(crate) fn join(mut self) -> Option<Box<dyn Any + Send>> {
        self.request_exit();
        if let Some(handle) = self.join_handle.take() {
            if let Err(error) = handle.join() {
                log::error!("RenderThread join returned Err: {error:?}");
            }
        }
        self.control.take_panic()
    }
}
