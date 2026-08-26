use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use truvis_render_loop::input_event::InputEvent;
use truvis_render_loop::renderer::Renderer;
use truvis_render_loop::{RenderLoop, RenderThreadControl, RenderThreadInit};

/// 只在 OS RenderThread 内调用一次的具体 Renderer 构造入口。
pub type RendererFactory = Box<dyn FnOnce() -> Box<dyn Renderer> + Send + 'static>;

type RenderThreadResult = Result<(), Box<dyn Any + Send>>;

/// standalone / embedded 窗口 owner 共用的 backend-independent 渲染线程宿主。
///
/// 本类型持有 OS thread handle、RenderLoop 控制契约和线程完成状态，但不接触任何
/// 具体窗口类型。窗口 owner 必须保证平台窗口在 [`Self::join`] 返回前保持存活。
pub struct RenderThread {
    control: Arc<RenderThreadControl>,

    /// 必须在线程发送完成通知前发布，避免 EventLoop 收到通知时线程尚未返回。
    finished: Arc<AtomicBool>,

    /// 内层 Result 保存 RenderLoop/Renderer panic，外层 JoinHandle::join 处理线程自身 panic。
    join_handle: Option<JoinHandle<RenderThreadResult>>,
}

impl RenderThread {
    /// 启动独立 OS 渲染线程，并在线程内依次构造窗口初始化参数与具体 Renderer。
    ///
    /// `build_init` 只能捕获 backend 已确认可跨线程传递的具体平台句柄；原始
    /// `RawWindowHandle` / `RawDisplayHandle` 在目标线程中重建，不跨线程移动。
    pub fn spawn<I, C>(
        initial_size: [u32; 2],
        build_init: I,
        renderer_factory: RendererFactory,
        on_finished: C,
    ) -> Result<Self, String>
    where
        I: FnOnce([u32; 2]) -> RenderThreadInit + Send + 'static,
        C: FnOnce() + Send + 'static,
    {
        let control = Arc::new(RenderThreadControl::new(initial_size));
        let finished = Arc::new(AtomicBool::new(false));
        let control_for_thread = Arc::clone(&control);
        let finished_for_thread = Arc::clone(&finished);

        let join_handle = thread::Builder::new()
            .name("RenderThread".to_string())
            .spawn(move || {
                let result = panic::catch_unwind(AssertUnwindSafe(|| {
                    let init = build_init(initial_size);
                    let renderer = renderer_factory();
                    RenderLoop::run(Arc::clone(&control_for_thread), init, renderer);
                }));

                if result.is_err() {
                    log::error!("RenderThread panicked; preserving payload for the window owner.");
                }

                control_for_thread.request_exit();
                finished_for_thread.store(true, Ordering::Release);
                on_finished();
                result
            })
            .map_err(|error| format!("failed to spawn RenderThread: {error}"))?;

        Ok(Self {
            control,
            finished,
            join_handle: Some(join_handle),
        })
    }

    /// 非阻塞通知 RenderLoop 退出；窗口 owner 通过单独的完成事件和 join 完成回收。
    pub fn request_exit(&self) {
        self.control.request_exit();
    }

    /// 发布最新窗口尺寸，同时保留 frame 契约既有 generation 与 debounce 语义。
    pub fn publish_resize(&self, size: [u32; 2]) {
        self.control.publish_resize(size);
    }

    /// 将 backend 已转换的输入事件交给 RenderLoop 的现有无界队列。
    pub fn send_input(&self, event: InputEvent) {
        self.control.send_input(event);
    }

    /// 读取先于窗口 EventLoop 完成通知发布的线程执行完成标记。
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    /// 等待 Vulkan owner 完整退出，并把渲染线程 panic 交给窗口 owner 处理。
    pub fn join(mut self) -> Option<Box<dyn Any + Send>> {
        self.join_inner()
    }

    fn join_inner(&mut self) -> Option<Box<dyn Any + Send>> {
        self.request_exit();
        self.join_handle.take().and_then(|handle| match handle.join() {
            Ok(Ok(())) => None,
            Ok(Err(payload)) | Err(payload) => Some(payload),
        })
    }
}

impl Drop for RenderThread {
    fn drop(&mut self) {
        if self.join_handle.is_some() && self.join_inner().is_some() {
            // Drop 只能覆盖窗口宿主异常退出的兜底路径，显式 join 才能把 payload 继续交给调用方。
            log::error!("RenderThread panicked while being joined from its Drop fallback.");
        }
    }
}
