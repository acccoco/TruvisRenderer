//! Windows 原生 child HWND 的 winit 宿主。
//!
//! 本模块只负责平台窗口、输入事件和 RenderThread 生命周期，不感知 Tauri、DOM、
//! WebView 或具体 App。调用方提供 parent raw handle，并保证 parent HWND 在
//! [`EmbeddedWinitHost::shutdown`] 返回前始终有效。

use std::panic;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use raw_window_handle::{HasWindowHandle, RawWindowHandle, Win32WindowHandle};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetWindowLongPtrW, HWND_TOP, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowLongPtrW,
    SetWindowPos, WS_CLIPSIBLINGS,
};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{DeviceEvent, DeviceId, ElementState, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::{Window, WindowId};

use truvis_app_frame::render_app_api::RenderApp;
use truvis_render_thread::{RenderAppFactory, RenderThread};

use crate::input_adapter::WinitInputAdapter;
use crate::win32::Win32RenderSurface;

/// DOM viewport 在 Tauri parent client area 中对应的物理像素矩形。
///
/// `x/y` 是相对 parent client origin 的位置，`width/height` 是 child HWND 的
/// client extent。零尺寸表示暂时隐藏窗口，并让现有 resize 路径进入 suspended 状态。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmbeddedViewportRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl EmbeddedViewportRect {
    fn checked_extent(self) -> Result<(i32, i32), String> {
        let width = i32::try_from(self.width).map_err(|_| "embedded viewport width exceeds i32".to_string())?;
        let height = i32::try_from(self.height).map_err(|_| "embedded viewport height exceeds i32".to_string())?;
        Ok((width, height))
    }
}

enum EmbeddedUserEvent {
    SetViewportRect(EmbeddedViewportRect),
    Shutdown,
    RenderFinished,
}

/// Tauri 等外层桌面壳持有的嵌入窗口控制句柄。
///
/// child HWND 和 winit `EventLoop` 位于 `RenderWindowThread`；本句柄只通过
/// `EventLoopProxy` 投递 latest viewport rect 或关闭请求。`shutdown` 会等待
/// RenderThread 先销毁全部 Vulkan/WSI 资源，再等待 child HWND 所在线程退出。
/// `spawn` 只同步等待 EventLoop 与 proxy 就绪；child HWND 必须在 Tauri setup
/// 返回、main thread 开始处理消息后异步创建。窗口创建前收到的 rect 会缓存并在
/// child ready 后应用。
pub struct EmbeddedWinitHost {
    event_proxy: EventLoopProxy<EmbeddedUserEvent>,
    window_thread: Option<JoinHandle<()>>,
}

impl EmbeddedWinitHost {
    pub fn spawn<F>(parent_window: RawWindowHandle, app_factory: F) -> Result<Self, String>
    where
        F: FnOnce() -> Box<dyn RenderApp> + Send + 'static,
    {
        let parent_window = Win32RenderSurface::require_window_handle(parent_window)?;
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let window_thread = thread::Builder::new()
            .name("RenderWindowThread".to_string())
            .spawn(move || {
                EmbeddedWinitHandler::run(parent_window, Box::new(app_factory), startup_sender);
            })
            .map_err(|error| format!("failed to spawn RenderWindowThread: {error}"))?;

        match startup_receiver.recv() {
            Ok(Ok(event_proxy)) => Ok(Self {
                event_proxy,
                window_thread: Some(window_thread),
            }),
            Ok(Err(error)) => {
                let _ = window_thread.join();
                Err(error)
            }
            Err(error) => {
                let join_error = window_thread.join().err();
                Err(format!("RenderWindowThread closed before startup completed: {error}; join={join_error:?}"))
            }
        }
    }

    pub fn set_viewport_rect(&self, rect: EmbeddedViewportRect) -> Result<(), String> {
        rect.checked_extent()?;
        self.event_proxy
            .send_event(EmbeddedUserEvent::SetViewportRect(rect))
            .map_err(|_| "RenderWindowThread is no longer available".to_string())
    }

    pub fn shutdown(mut self) -> Result<(), String> {
        self.shutdown_inner()
    }

    fn shutdown_inner(&mut self) -> Result<(), String> {
        if self.window_thread.is_none() {
            return Ok(());
        }
        let _ = self.event_proxy.send_event(EmbeddedUserEvent::Shutdown);
        if let Some(window_thread) = self.window_thread.take() {
            window_thread.join().map_err(|payload| format!("RenderWindowThread panicked: {payload:?}"))?;
        }
        Ok(())
    }
}

impl Drop for EmbeddedWinitHost {
    fn drop(&mut self) {
        // 主窗口 CloseRequested 是正常关闭路径；此处覆盖 app.exit、setup 后异常等
        // 兜底路径，仍保持 Vulkan surface → child HWND → parent HWND 的销毁顺序。
        if let Err(error) = self.shutdown_inner() {
            log::error!("failed to shut down embedded winit host during drop: {error}");
        }
    }
}

/// `RenderWindowThread` 内部的 winit application handler。
///
/// 本类型拥有 child `Window` 和 [`RenderThread`]。它接收的 viewport rect 只是
/// 平台窗口几何命令；RenderThread 不接收该命令，而是继续通过 child HWND 产生的
/// `WindowEvent::Resized` 与现有 resize generation 感知变化。
struct EmbeddedWinitHandler {
    parent_window: RawWindowHandle,
    app_factory: Option<RenderAppFactory>,

    /// 字段顺序保证异常展开时先触发 RenderThread 的 join fallback，再 drop child Window。
    render_thread: Option<RenderThread>,

    window: Option<Window>,
    event_proxy: EventLoopProxy<EmbeddedUserEvent>,
    pending_viewport_rect: EmbeddedViewportRect,
}

impl EmbeddedWinitHandler {
    fn run(
        parent_window: Win32WindowHandle,
        app_factory: RenderAppFactory,
        startup_sender: mpsc::SyncSender<Result<EventLoopProxy<EmbeddedUserEvent>, String>>,
    ) {
        let mut event_loop_builder = winit::event_loop::EventLoop::<EmbeddedUserEvent>::with_user_event();
        // Tauri/Tao 已经拥有进程级 DPI 策略；embedded winit 只消费窗口 DPI 事件，
        // 避免两个 UI runtime 重复修改 process-wide DPI awareness。
        event_loop_builder.with_any_thread(true).with_dpi_aware(false);
        let event_loop = match event_loop_builder.build() {
            Ok(event_loop) => event_loop,
            Err(error) => {
                let _ = startup_sender.send(Err(format!("failed to create embedded winit EventLoop: {error}")));
                return;
            }
        };
        let event_proxy = event_loop.create_proxy();
        // Windows 在跨线程 parent 上创建 child HWND 时会向 parent thread 同步发送
        // 窗口消息。这里必须先把 proxy 交还 Tauri setup，让 main thread 离开同步等待
        // 并进入 Tao 消息泵，再由 run_app/resumed 创建 child，否则会形成跨线程死锁。
        if startup_sender.send(Ok(event_proxy.clone())).is_err() {
            return;
        }
        let mut handler = Self {
            parent_window: RawWindowHandle::Win32(parent_window),
            app_factory: Some(app_factory),
            render_thread: None,
            window: None,
            event_proxy,
            pending_viewport_rect: EmbeddedViewportRect::default(),
        };

        if let Err(error) = event_loop.run_app(&mut handler) {
            log::error!("embedded winit event loop failed: {error}");
        }
        let panic_payload = handler.destroy();
        if let Some(payload) = panic_payload {
            panic::resume_unwind(payload);
        }
    }

    fn init_after_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let attributes = Window::default_attributes()
            .with_title("Render Viewport")
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(false)
            .with_visible(false)
            .with_position(PhysicalPosition::new(0, 0))
            .with_inner_size(PhysicalSize::new(1, 1));
        // Safety: Tauri desktop state guarantees the supplied parent HWND remains alive until
        // EmbeddedWinitHost::shutdown has joined this window thread.
        let attributes = unsafe { attributes.with_parent_window(Some(self.parent_window)) };
        let window = event_loop
            .create_window(attributes)
            .map_err(|error| format!("failed to create embedded render child window: {error}"))?;
        Self::configure_child_style(&window)?;
        self.window = Some(window);
        self.apply_viewport_rect_and_start_render_thread()
    }

    /// 把 DOM 首次给出的非零 rect 应用到 child 后再启动 RenderThread。
    ///
    /// child 初始创建为隐藏的 1x1 窗口；若先启动 RenderThread，初始化参数会把
    /// 1x1 传给 App/Plugin。即使 Vulkan surface 随后已经读到真实 extent，首次
    /// swapchain 也可能无需再次重建，从而不会补发完整 plugin resize。这里让
    /// `Window::inner_size` 在 RenderThread 创建前就反映 DOM 物理尺寸，确保 App、GUI、
    /// swapchain 从同一个初始 extent 开始。
    fn apply_viewport_rect_and_start_render_thread(&mut self) -> Result<(), String> {
        let window = self.window.as_ref().ok_or_else(|| "embedded child window is not available".to_string())?;
        Self::apply_viewport_rect(window, self.pending_viewport_rect)?;

        if self.pending_viewport_rect.width == 0
            || self.pending_viewport_rect.height == 0
            || self.render_thread.is_some()
        {
            return Ok(());
        }

        let factory = self.app_factory.take().ok_or_else(|| "render app factory already consumed".to_string())?;
        let surface = Win32RenderSurface::from_window(window)?;
        let initial_size = surface.initial_size();
        let event_proxy = self.event_proxy.clone();
        let render_thread = RenderThread::spawn(
            initial_size,
            move |size| surface.into_render_thread_init(size),
            factory,
            move || {
                let _ = event_proxy.send_event(EmbeddedUserEvent::RenderFinished);
            },
        )?;
        self.render_thread = Some(render_thread);
        Ok(())
    }

    fn configure_child_style(window: &Window) -> Result<(), String> {
        let hwnd = Self::window_hwnd(window)?;
        // Safety: hwnd 来自仍由本线程持有的 winit Window。这里只补充 sibling clipping，
        // 不改变 parent、WndProc 或 userdata。
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            SetWindowLongPtrW(hwnd, GWL_STYLE, (style | WS_CLIPSIBLINGS) as isize);
        }
        Ok(())
    }

    /// 鼠标按下明确表示用户要与 native viewport 交互，此时把 keyboard focus
    /// 从 WebView sibling 转给 render child。winit 的 Windows backend 会消费
    /// `WM_*BUTTONDOWN` 并发送 `MouseInput`，但嵌入式 child 不会因此自动获得焦点；
    /// 只有在持有 child HWND 的窗口线程调用 `SetFocus`，后续键盘消息才会沿现有
    /// `WinitInputAdapter` 输入链路进入相机控制。用户点击周围 WebView 控件时，
    /// WebView 会按 Windows 默认行为重新取得焦点，因此字符输入无需额外转发。
    fn focus_child_window(window: &Window) -> Result<(), String> {
        let hwnd = Self::window_hwnd(window)?;
        // Safety: hwnd 属于当前 RenderWindowThread，且调用发生在该线程处理
        // `WindowEvent::MouseInput` 的消息循环中，满足 SetFocus 的线程归属要求。
        unsafe {
            SetFocus(hwnd);
            if GetFocus() != hwnd {
                return Err("SetFocus did not assign keyboard focus to the embedded render child".to_string());
            }
        }
        Ok(())
    }

    fn apply_viewport_rect(window: &Window, rect: EmbeddedViewportRect) -> Result<(), String> {
        let hwnd = Self::window_hwnd(window)?;
        let (width, height) = rect.checked_extent()?;
        let visibility_flag = if rect.width == 0 || rect.height == 0 { SWP_HIDEWINDOW } else { SWP_SHOWWINDOW };
        // Safety: hwnd 属于当前 RenderWindowThread；SetWindowPos 同时提交位置、尺寸和
        // sibling Z-order，Windows 随后产生正常 WM_SIZE，由 winit 转换为 Resized。
        let succeeded =
            unsafe { SetWindowPos(hwnd, HWND_TOP, rect.x, rect.y, width, height, SWP_NOACTIVATE | visibility_flag) };
        if succeeded == 0 {
            return Err(format!("SetWindowPos failed: {}", std::io::Error::last_os_error()));
        }
        Ok(())
    }

    fn window_hwnd(window: &Window) -> Result<HWND, String> {
        let handle =
            window.window_handle().map_err(|error| format!("failed to read embedded child HWND: {error}"))?.as_raw();
        match handle {
            RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get() as HWND),
            other => Err(format!("embedded Windows host received non-Win32 window handle: {other:?}")),
        }
    }

    fn destroy(&mut self) -> Option<Box<dyn std::any::Any + Send>> {
        let panic_payload = self.render_thread.take().and_then(RenderThread::join);
        self.window = None;
        panic_payload
    }

    fn fail_startup(&mut self, event_loop: &ActiveEventLoop, error: String) {
        log::error!("failed to initialize embedded render child: {error}");
        event_loop.exit();
    }
}

impl ApplicationHandler<EmbeddedUserEvent> for EmbeddedWinitHandler {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: StartCause) {}

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        match self.init_after_window(event_loop) {
            Ok(()) => {}
            Err(error) => self.fail_startup(event_loop, error),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: EmbeddedUserEvent) {
        match event {
            EmbeddedUserEvent::SetViewportRect(rect) => {
                self.pending_viewport_rect = rect;
                if self.window.is_some() {
                    if let Err(error) = self.apply_viewport_rect_and_start_render_thread() {
                        self.fail_startup(
                            event_loop,
                            format!("failed to update embedded viewport rect {rect:?}: {error}"),
                        );
                    }
                }
            }
            EmbeddedUserEvent::Shutdown => {
                if let Some(render_thread) = self.render_thread.as_ref() {
                    render_thread.request_exit();
                } else {
                    event_loop.exit();
                }
            }
            EmbeddedUserEvent::RenderFinished => event_loop.exit(),
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        let Some(render_thread) = self.render_thread.as_ref() else {
            return;
        };

        match &event {
            WindowEvent::CloseRequested => render_thread.request_exit(),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            } => {
                if let Some(window) = self.window.as_ref()
                    && let Err(error) = Self::focus_child_window(window)
                {
                    log::warn!("failed to focus embedded render child after mouse press: {error}");
                }
            }
            WindowEvent::Resized(size) => {
                render_thread.publish_resize([size.width, size.height]);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                log::info!("embedded render viewport scale factor changed to {scale_factor}");
            }
            _ => {}
        }

        let input_event = WinitInputAdapter::from_winit_event(&event);
        use truvis_app_frame::input_event::InputEvent;
        match input_event {
            InputEvent::Other | InputEvent::Resized { .. } => {}
            _ => {
                render_thread.send_input(input_event);
            }
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: DeviceId, _event: DeviceEvent) {}

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.render_thread.as_ref().is_some_and(RenderThread::is_finished) {
            event_loop.exit();
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        log::warn!("embedded winit event: suspended");
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        log::info!("embedded winit event loop exiting");
    }

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
        log::warn!("embedded winit memory warning");
    }
}
