use std::time::Duration;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Icon, Window, WindowId};

use truvis_app_frame::input_event::InputEvent;
use truvis_app_frame::render_app_api::RenderApp;
use truvis_render_thread::{RenderAppFactory, RenderThread};

use crate::input_adapter::WinitInputAdapter;
use crate::win32::Win32RenderSurface;

enum UserEvent {
    RenderFinished,
}

/// 由具体 App 入口决定的 standalone 窗口策略；平台宿主不读取产品资源或路径。
pub struct StandaloneWindowOptions {
    pub title: String,
    pub logical_size: [f64; 2],
    pub transparent: bool,
    pub icon_bytes: Option<Vec<u8>>,
}

/// standalone main thread 上的 winit 窗口 owner 和事件循环宿主。
///
/// 本类型始终先等待 RenderThread 完整销毁 Vulkan surface，再允许对应 Window drop。
pub struct StandaloneWinitHost {
    options: StandaloneWindowOptions,
    app_factory: Option<RenderAppFactory>,

    /// 字段顺序保证异常展开时先触发 RenderThread 的 join fallback，再 drop Window。
    render_thread: Option<RenderThread>,

    window: Option<Window>,
    event_proxy: EventLoopProxy<UserEvent>,
}

impl StandaloneWinitHost {
    /// 启动窗口事件循环；具体 App 只会在 OS RenderThread 内构造一次。
    pub fn run<F>(options: StandaloneWindowOptions, app_factory: F)
    where
        F: FnOnce() -> Box<dyn RenderApp> + Send + 'static,
    {
        let mut event_loop = winit::event_loop::EventLoop::<UserEvent>::with_user_event().build().unwrap();
        let event_proxy = event_loop.create_proxy();

        let mut host = Self {
            options,
            app_factory: Some(Box::new(app_factory)),
            render_thread: None,
            window: None,
            event_proxy,
        };

        loop {
            match event_loop.pump_app_events(Some(Duration::from_millis(8)), &mut host) {
                PumpStatus::Continue => {}
                PumpStatus::Exit(exit_code) => {
                    assert_eq!(exit_code, 0, "standalone winit event loop exited with a non-zero code");
                    break;
                }
            }
        }
        drop(event_loop);
        log::info!("standalone winit event loop finished");
        host.destroy();
    }

    fn create_window(&self, event_loop: &ActiveEventLoop) -> Window {
        let [width, height] = self.options.logical_size;
        let mut attributes = Window::default_attributes()
            .with_title(&self.options.title)
            .with_transparent(self.options.transparent)
            .with_inner_size(LogicalSize::new(width, height));

        if let Some(icon_bytes) = self.options.icon_bytes.as_deref() {
            let icon = Self::decode_icon(icon_bytes);
            attributes = attributes.with_window_icon(Some(icon.clone())).with_taskbar_icon(Some(icon));
        }

        event_loop.create_window(attributes).unwrap()
    }

    fn decode_icon(bytes: &[u8]) -> Icon {
        let image = image::load_from_memory(bytes).expect("failed to decode standalone window icon").into_rgba8();
        let (width, height) = image.dimensions();
        Icon::from_rgba(image.into_raw(), width, height).expect("failed to create standalone window icon")
    }

    fn init_after_window(&mut self, event_loop: &ActiveEventLoop) {
        let window = self.create_window(event_loop);
        let surface = Win32RenderSurface::from_window(&window).unwrap_or_else(|error| panic!("{error}"));
        let initial_size = surface.initial_size();
        let factory = self.app_factory.take().expect("app_factory already consumed");
        let event_proxy = self.event_proxy.clone();
        let render_thread = RenderThread::spawn(
            initial_size,
            move |size| surface.into_render_thread_init(size),
            factory,
            move || {
                let _ = event_proxy.send_event(UserEvent::RenderFinished);
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));

        self.window = Some(window);
        self.render_thread = Some(render_thread);
    }

    fn destroy(mut self) {
        let panic_payload = self.render_thread.take().and_then(RenderThread::join);
        self.window = None;
        if let Some(payload) = panic_payload {
            std::panic::resume_unwind(payload);
        }
    }
}

impl ApplicationHandler<UserEvent> for StandaloneWinitHost {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: StartCause) {}

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        assert!(self.window.is_none(), "window should be None when resumed.");
        log::info!("winit event: resumed");
        self.init_after_window(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::RenderFinished => event_loop.exit(),
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        let Some(render_thread) = self.render_thread.as_ref() else {
            return;
        };

        match &event {
            WindowEvent::CloseRequested => render_thread.request_exit(),
            WindowEvent::Resized(size) => render_thread.publish_resize([size.width, size.height]),
            WindowEvent::ScaleFactorChanged { .. } => {}
            _ => {}
        }

        let input_event = WinitInputAdapter::from_winit_event(&event);
        match input_event {
            InputEvent::Other | InputEvent::Resized { .. } => {}
            _ => render_thread.send_input(input_event),
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: DeviceId, _event: DeviceEvent) {}

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.render_thread.as_ref().is_some_and(RenderThread::is_finished) {
            event_loop.exit();
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
