use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use truvis_frame_api::frame_app::{FrameApp, FrameAppHooks};
use truvis_frame_api::input_event::InputEvent;
use truvis_render_backend::render_backend::{RenderBackendInitCtx, RenderBackendResizeCtx};

use crate::base_app::BaseApp;

/// [`FrameAppShell`] 传给 app state 的窗口绑定初始化上下文。
pub struct FrameAppInitCtx<'a> {
    pub backend: RenderBackendInitCtx<'a>,
    pub scale_factor: f64,
    pub window_size: [u32; 2],
}

/// swapchain 资源变化时，[`FrameAppShell`] 传给 app state 的 resize 上下文。
pub struct FrameAppResizeCtx<'a> {
    pub backend: RenderBackendResizeCtx<'a>,
    pub window_size: [u32; 2],
}

/// 由 [`FrameAppShell`] 驱动的具体 app state。
///
/// App state 持有 GUI、camera/input state、overlay 和 render plugin。
/// shell 持有 `BaseApp`，并为 render loop 实现 object-safe 的 `FrameApp` 契约。
pub trait FrameAppState: FrameAppHooks {
    fn init(&mut self, ctx: FrameAppInitCtx<'_>);

    fn on_resize(&mut self, _ctx: FrameAppResizeCtx<'_>) {}

    fn shutdown(&mut self) {}
}

/// 将具体 app state 转换为 render-loop [`FrameApp`] 的适配器。
pub struct FrameAppShell<A> {
    base: Option<BaseApp>,
    app: A,
}

impl<A> FrameAppShell<A> {
    pub fn new(app: A) -> Self {
        Self { base: None, app }
    }

    pub fn app(&self) -> &A {
        &self.app
    }

    pub fn app_mut(&mut self) -> &mut A {
        &mut self.app
    }
}

impl<A> FrameApp for FrameAppShell<A>
where
    A: FrameAppState,
{
    fn init_after_window(
        &mut self,
        raw_display: RawDisplayHandle,
        raw_window: RawWindowHandle,
        scale_factor: f64,
        window_size: [u32; 2],
    ) {
        let mut base = BaseApp::new(raw_display);
        {
            let backend = base.init_after_window(raw_display, raw_window, window_size);
            self.app.init(FrameAppInitCtx {
                backend,
                scale_factor,
                window_size,
            });
        }
        self.base = Some(base);
    }

    fn run_frame(&mut self) {
        let Self { base, app } = self;
        let base = base.as_mut().expect("BaseApp missing in FrameAppShell::run_frame");
        base.run_frame(app);
    }

    fn push_input_event(&mut self, event: InputEvent) {
        self.base.as_mut().expect("BaseApp missing in FrameAppShell::push_input_event").push_input_event(event);
    }

    fn recreate_swapchain_if_needed(&mut self, new_size: [u32; 2]) {
        let Self { base, app } = self;
        let Some(backend) = base
            .as_mut()
            .expect("BaseApp missing in FrameAppShell::recreate_swapchain_if_needed")
            .recreate_swapchain_if_needed(new_size)
        else {
            return;
        };

        app.on_resize(FrameAppResizeCtx {
            backend,
            window_size: new_size,
        });
    }

    fn time_to_render(&self) -> bool {
        self.base.as_ref().expect("BaseApp missing in FrameAppShell::time_to_render").time_to_render()
    }

    fn shutdown(&mut self) {
        self.app.shutdown();
        if let Some(base) = self.base.take() {
            base.destroy();
        }
    }
}
