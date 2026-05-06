//! 渲染线程和 BaseApp 帧骨架使用的 App 契约。

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use truvis_render_backend::platform::camera::Camera;
use truvis_render_backend::render_backend::{RenderBackendRenderCtx, RenderBackendUpdateCtx};

use crate::input_event::InputEvent;

/// 由 render loop 驱动的 object-safe 外部契约。
pub trait FrameApp {
    fn init_after_window(
        &mut self,
        raw_display: RawDisplayHandle,
        raw_window: RawWindowHandle,
        scale_factor: f64,
        window_size: [u32; 2],
    );

    fn run_frame(&mut self);

    fn push_input_event(&mut self, event: InputEvent);

    fn recreate_swapchain_if_needed(&mut self, new_size: [u32; 2]);

    fn time_to_render(&self) -> bool;

    fn shutdown(&mut self);
}

/// `BaseApp::run_frame` 调用的内部 hook 点。
pub trait FrameAppHooks {
    fn on_input(&mut self, events: &[InputEvent]);

    fn update(&mut self, ctx: &mut RenderBackendUpdateCtx);

    fn render(&mut self, ctx: &RenderBackendRenderCtx);

    fn camera(&self) -> &Camera;
}
