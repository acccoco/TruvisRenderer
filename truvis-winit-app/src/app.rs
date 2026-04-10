use crate::winit_event_adapter::WinitEventAdapter;
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use truvis_app::outer_app::base::OuterApp;
use truvis_app::render_app::RenderApp;
use truvis_path::TruvisPath;
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::Window;
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, StartCause, WindowEvent},
    event_loop::ActiveEventLoop,
    window::WindowId,
};

pub struct UserEvent;

pub struct WinitApp {
    render_app: RenderApp,

    window: Option<Window>,
}
// 总的 main 函数
impl WinitApp {
    /// 整个程序的入口
    pub fn run(outer_app: Box<dyn OuterApp>) {
        RenderApp::init_env();

        let event_loop = winit::event_loop::EventLoop::<UserEvent>::with_user_event().build().unwrap();

        let mut app = Self {
            render_app: RenderApp::new(event_loop.raw_display_handle().unwrap(), outer_app),
            window: None,
        };

        event_loop.run_app(&mut app).unwrap();

        log::info!("end run.");

        app.destroy();
    }
}
// new & init
impl WinitApp {
    /// 在 window 创建之后调用，初始化 Renderer 和 GUI
    fn init_after_window(&mut self, event_loop: &ActiveEventLoop) {
        let window = Self::create_window(event_loop, "Truvis".to_string(), [1200.0, 800.0]);

        let window_size = window.inner_size();

        self.render_app.init_after_window(
            window.raw_display_handle().unwrap(),
            window.raw_window_handle().unwrap(),
            window.scale_factor(),
            [window_size.width, window_size.height],
        );

        self.window = Some(window);
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
}
// destroy
impl WinitApp {
    fn destroy(mut self) {
        self.render_app.destroy();
        self.window = None;
    }
}
// 各种 winit 的事件处理
impl ApplicationHandler<UserEvent> for WinitApp {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: StartCause) {
        // TODO 确认一下发送时机
        // TODO 可以在此处更新 timer
    }

    // 建议在这里创建 window 和 Renderer
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        assert!(self.window.is_none(), "window should be None when resumed.");

        log::info!("winit event: resumed");

        self.init_after_window(event_loop);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: UserEvent) {
        todo!()
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        let input_event = WinitEventAdapter::from_winit_event(&event);
        self.render_app.handle_event(&input_event);

        // TODO 可以放到 render app 里面去处理，加入队列中
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                self.render_app.big_update();
                // TODO 是否应该手动调用 redraw，实现死循环？
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: DeviceId, _event: DeviceEvent) {
        // 使用InputManager处理设备事件
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        self.window.as_ref().unwrap().request_redraw();
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
