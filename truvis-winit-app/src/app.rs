use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::{self, JoinHandle};

use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use truvis_app::app_plugin::{AppPlugin, LegacyOuterAppAdapter};
use truvis_app::frame_runtime::FrameRuntime;
#[allow(deprecated)]
use truvis_app::outer_app::base::OuterApp;
use truvis_path::TruvisPath;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Window, WindowId};

use crate::render_loop::render_loop;
use crate::shared::{RenderInitMsg, SendWrapper, SharedState, pack_size};
use crate::winit_event_adapter::WinitEventAdapter;

pub struct UserEvent;

/// AppPlugin 工厂闭包类型（新契约路径）。
pub type AppPluginFactory = Box<dyn FnOnce() -> Box<dyn AppPlugin> + Send + 'static>;

/// OuterApp 工厂闭包类型（兼容路径）。
#[deprecated(note = "Use `AppPluginFactory` / `WinitApp::run_plugin`. This alias will be removed after the compatibility window.")]
#[allow(deprecated)]
pub type OuterAppFactory = Box<dyn FnOnce() -> Box<dyn OuterApp> + Send + 'static>;

/// winit 主线程的 app handler。
///
/// 生命周期：
/// - `resumed` 中创建 `Window` 并 spawn 渲染线程；
/// - `window_event` 将事件转发到 channel、resize 写入 atomic、`CloseRequested` 设置 exit；
/// - `about_to_wait` 观察到 `render_finished` 时调 `event_loop.exit()`；
/// - `run_app` 返回后 `destroy()` join 渲染线程并在最后 drop `Window`。
pub struct WinitApp {
    window: Option<Window>,

    shared: Option<Arc<SharedState>>,

    /// 仅在 `resumed` 前有值；spawn 时 `take()` 后交给渲染线程。
    plugin_factory: Option<AppPluginFactory>,

    render_thread: Option<JoinHandle<()>>,
}

impl WinitApp {
    /// 新契约入口。`plugin_factory` 在渲染线程上调用一次，返回的
    /// [`AppPlugin`] 直接接入 `FrameRuntime`。
    pub fn run_plugin<F>(plugin_factory: F)
    where
        F: FnOnce() -> Box<dyn AppPlugin> + Send + 'static,
    {
        Self::run_inner(Box::new(plugin_factory));
    }

    /// 兼容入口：接受旧 [`OuterApp`] 工厂，内部包装为 [`LegacyOuterAppAdapter`]。
    #[deprecated(note = "Use `run_plugin` with an `AppPlugin` implementation.")]
    #[allow(deprecated)]
    pub fn run<F>(outer_app_factory: F)
    where
        F: FnOnce() -> Box<dyn OuterApp> + Send + 'static,
    {
        Self::run_inner(Box::new(move || -> Box<dyn AppPlugin> {
            Box::new(LegacyOuterAppAdapter::new(outer_app_factory()))
        }));
    }

    fn run_inner(plugin_factory: AppPluginFactory) {
        FrameRuntime::init_env();

        let event_loop = winit::event_loop::EventLoop::<UserEvent>::with_user_event().build().unwrap();

        let mut app = Self {
            window: None,
            shared: None,
            plugin_factory: Some(plugin_factory),
            render_thread: None,
        };

        event_loop.run_app(&mut app).unwrap();

        log::info!("end run.");

        app.destroy();
    }

    fn create_window(event_loop: &ActiveEventLoop, window_title: String, window_extent: [f64; 2]) -> Window {
        fn load_icon(bytes: &[u8]) -> winit::window::Icon {
            let (icon_rgba, icon_width, icon_height) = {
                let image = image::load_from_memory(bytes).unwrap().into_rgba8();
                let (width, height) = image.dimensions();
                let rgba = image.into_raw();
                (rgba, width, height)
            };
            winit::window::Icon::from_rgba(icon_rgba, icon_width, icon_height).expect("Failed to open icon")
        }

        let icon_data =
            std::fs::read(TruvisPath::resources_path_str("DruvisIII.png")).expect("Failed to read icon file");
        let icon = load_icon(icon_data.as_ref());
        let window_attr = Window::default_attributes()
            .with_title(window_title)
            .with_window_icon(Some(icon.clone()))
            .with_taskbar_icon(Some(icon.clone()))
            .with_transparent(true)
            .with_inner_size(winit::dpi::LogicalSize::new(window_extent[0], window_extent[1]));

        event_loop.create_window(window_attr).unwrap()
    }

    /// 在 `resumed` 中创建 window 并 spawn 渲染线程。
    fn init_after_window(&mut self, event_loop: &ActiveEventLoop) {
        let window = Self::create_window(event_loop, "Truvis".to_string(), [1200.0, 800.0]);
        let window_size = window.inner_size();
        let initial_size = [window_size.width, window_size.height];

        let shared = Arc::new(SharedState::new(initial_size));

        let init_msg = RenderInitMsg {
            raw_display: SendWrapper(window.raw_display_handle().unwrap()),
            raw_window: SendWrapper(window.raw_window_handle().unwrap()),
            scale_factor: window.scale_factor(),
            initial_size,
        };

        let factory = self.plugin_factory.take().expect("plugin_factory already consumed");
        let shared_for_thread = shared.clone();

        let join_handle = thread::Builder::new()
            .name("RenderThread".to_string())
            .spawn(move || {
                let shared_in_thread = shared_for_thread;
                let result = panic::catch_unwind(AssertUnwindSafe(|| {
                    let plugin = factory();
                    render_loop(shared_in_thread.clone(), init_msg, plugin);
                }));
                // 无论 panic 与否，都必须让主线程观察到退出信号，否则 about_to_wait 死循环
                if let Err(payload) = result {
                    log::error!("RenderThread panicked; capturing payload for main thread resume.");
                    if let Ok(mut slot) = shared_in_thread.panic_payload.lock() {
                        *slot = Some(payload);
                    }
                }
                shared_in_thread.exit.store(true, Ordering::Release);
                shared_in_thread.render_finished.store(true, Ordering::Release);
            })
            .expect("failed to spawn RenderThread");

        self.window = Some(window);
        self.shared = Some(shared);
        self.render_thread = Some(join_handle);
    }

    /// `run_app` 返回后调用：先 join 渲染线程（此时 Vulkan 资源已销毁），
    /// 再 drop window；最后若渲染线程曾 panic 则 resume 到主线程。
    fn destroy(mut self) {
        if let Some(handle) = self.render_thread.take() {
            if let Err(e) = handle.join() {
                log::error!("RenderThread join returned Err: {:?}", e);
            }
        }

        // Window 必须在 surface 销毁（渲染线程销毁 Renderer 时完成）之后 drop。
        self.window = None;

        if let Some(shared) = self.shared.take() {
            let payload = shared.panic_payload.lock().ok().and_then(|mut g| g.take());
            if let Some(payload) = payload {
                panic::resume_unwind(payload);
            }
        }
    }
}

impl ApplicationHandler<UserEvent> for WinitApp {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: StartCause) {}

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        assert!(self.window.is_none(), "window should be None when resumed.");
        log::info!("winit event: resumed");
        self.init_after_window(event_loop);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: UserEvent) {
        todo!()
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        let Some(shared) = self.shared.as_ref() else {
            return;
        };

        // resize / close 优先处理共享状态，不经过事件通道
        match &event {
            WindowEvent::CloseRequested => {
                // 二阶段关闭：只置 exit，不在此调用 event_loop.exit()
                shared.exit.store(true, Ordering::Release);
            }
            WindowEvent::Resized(size) => {
                shared.size.store(pack_size(size.width, size.height), Ordering::Relaxed);
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                // winit 会额外发 Resized 事件，此处无需独立处理
            }
            _ => {}
        }

        // 其它输入事件转发到渲染线程；Resized/Other 不经通道（Resized 走 atomic）
        let input_event = WinitEventAdapter::from_winit_event(&event);
        use truvis_app::platform::input_event::InputEvent;
        match input_event {
            InputEvent::Other | InputEvent::Resized { .. } => {}
            _ => {
                // unbounded send 在主线程上非阻塞
                let _ = shared.event_sender.send(input_event);
            }
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: DeviceId, _event: DeviceEvent) {}

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // 渲染线程自驱，不再 request_redraw
        if let Some(shared) = self.shared.as_ref() {
            if shared.render_finished.load(Ordering::Acquire) {
                event_loop.exit();
            }
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        log::warn!("winit event: suspended");
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        log::info!("loop exiting");
    }

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
        log::warn!("memory warning");
    }
}
