use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::{self, JoinHandle};

use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use truvis_app_api::app_plugin::AppPlugin;
use truvis_frame_runtime::FrameRuntime;
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

type AppPluginFactory = Box<dyn FnOnce() -> Box<dyn AppPlugin> + Send + 'static>;

/// winit main-thread app handler.
pub struct WinitApp {
    window: Option<Window>,
    shared: Option<Arc<SharedState>>,
    plugin_factory: Option<AppPluginFactory>,
    render_thread: Option<JoinHandle<()>>,
}

impl WinitApp {
    /// Primary entry point. `plugin_factory` is called once on the render thread.
    pub fn run_plugin<F>(plugin_factory: F)
    where
        F: FnOnce() -> Box<dyn AppPlugin> + Send + 'static,
    {
        Self::run_inner(Box::new(plugin_factory));
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

    fn destroy(mut self) {
        if let Some(handle) = self.render_thread.take() {
            if let Err(e) = handle.join() {
                log::error!("RenderThread join returned Err: {:?}", e);
            }
        }

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

        match &event {
            WindowEvent::CloseRequested => {
                shared.exit.store(true, Ordering::Release);
            }
            WindowEvent::Resized(size) => {
                shared.size.store(pack_size(size.width, size.height), Ordering::Relaxed);
            }
            WindowEvent::ScaleFactorChanged { .. } => {}
            _ => {}
        }

        let input_event = WinitEventAdapter::from_winit_event(&event);
        use truvis_app_api::input_event::InputEvent;
        match input_event {
            InputEvent::Other | InputEvent::Resized { .. } => {}
            _ => {
                let _ = shared.event_sender.send(input_event);
            }
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: DeviceId, _event: DeviceEvent) {}

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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
