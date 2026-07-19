use std::sync::atomic::Ordering;

use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Window, WindowId};

use truvis_app_frame::render_app_api::RenderApp;
use truvis_app_frame::{init_env_with_log_file, pack_size};
use truvis_logs::LogFilePath;
use truvis_path::TruvisPath;

use crate::render_worker::{RenderAppFactory, RenderWorker};
use crate::winit_event_adapter::WinitEventAdapter;

enum UserEvent {
    RenderFinished,
}

/// winit 主线程 app handler。
pub struct WinitApp {
    window: Option<Window>,
    app_factory: Option<RenderAppFactory>,
    render_worker: Option<RenderWorker>,
    event_proxy: EventLoopProxy<UserEvent>,
}

impl WinitApp {
    /// 主入口。`app_factory` 会在渲染线程上调用一次。
    pub fn run_app<F>(app_factory: F)
    where
        F: FnOnce() -> Box<dyn RenderApp> + Send + 'static,
    {
        Self::run_inner(Box::new(app_factory));
    }

    fn run_inner(app_factory: RenderAppFactory) {
        init_env_with_log_file(LogFilePath::current_exe(TruvisPath::temp_dir()));

        let event_loop = winit::event_loop::EventLoop::<UserEvent>::with_user_event().build().unwrap();
        let event_proxy = event_loop.create_proxy();

        let mut app = Self {
            window: None,
            app_factory: Some(app_factory),
            render_worker: None,
            event_proxy,
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

    fn init_after_window(&mut self, event_loop: &ActiveEventLoop) {
        let window = Self::create_window(event_loop, "Truvis".to_string(), [1200.0, 800.0]);
        let factory = self.app_factory.take().expect("app_factory already consumed");
        let event_proxy = self.event_proxy.clone();
        let render_worker = RenderWorker::spawn(&window, factory, move || {
            let _ = event_proxy.send_event(UserEvent::RenderFinished);
        })
        .unwrap_or_else(|error| panic!("{error}"));

        self.window = Some(window);
        self.render_worker = Some(render_worker);
    }

    fn destroy(mut self) {
        let panic_payload = self.render_worker.take().and_then(RenderWorker::finish);
        self.window = None;
        if let Some(payload) = panic_payload {
            std::panic::resume_unwind(payload);
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

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::RenderFinished => event_loop.exit(),
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        let Some(worker) = self.render_worker.as_ref() else {
            return;
        };
        let shared = worker.shared();

        match &event {
            WindowEvent::CloseRequested => {
                worker.request_exit();
            }
            WindowEvent::Resized(size) => {
                shared.size.store(pack_size(size.width, size.height), Ordering::Relaxed);
                shared.resize_generation.fetch_add(1, Ordering::Release);
            }
            WindowEvent::ScaleFactorChanged { .. } => {}
            _ => {}
        }

        let input_event = WinitEventAdapter::from_winit_event(&event);
        use truvis_app_frame::input_event::InputEvent;
        match input_event {
            InputEvent::Other | InputEvent::Resized { .. } => {}
            _ => {
                let _ = shared.event_sender.send(input_event);
            }
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: DeviceId, _event: DeviceEvent) {}

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(worker) = self.render_worker.as_ref() {
            if worker.shared().render_finished.load(Ordering::Acquire) {
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
