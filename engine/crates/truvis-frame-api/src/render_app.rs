//! 渲染线程和 RenderAppShell 帧骨架使用的 App 契约。

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use truvis_render_backend::platform::camera::Camera;
use truvis_render_backend::render_backend::{
    RenderBackendInitCtx, RenderBackendRenderCtx, RenderBackendResizeCtx, RenderBackendUpdateCtx,
};

use crate::input_event::InputEvent;

/// 由 render loop 驱动的 object-safe 外部契约。
pub trait RenderApp {
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

/// `RenderAppShell` 传给 app hooks 的窗口绑定初始化上下文。
pub struct RenderAppInitCtx<'a> {
    pub backend: RenderBackendInitCtx<'a>,
    pub scale_factor: f64,
    pub window_size: [u32; 2],
}

/// swapchain 资源变化时，`RenderAppShell` 传给 app hooks 的 resize 上下文。
pub struct RenderAppResizeCtx<'a> {
    pub backend: RenderBackendResizeCtx<'a>,
    pub window_size: [u32; 2],
}

/// 由 `RenderAppShell` 驱动的具体 app hook 契约。
///
/// 具体 app 持有 GUI、camera/input state、overlay 和 render plugin。
/// shell 持有 RenderBackend 与输入队列，并通过这些 hook 交出生命周期和帧阶段控制点。
pub trait RenderAppHooks {
    fn init(&mut self, ctx: RenderAppInitCtx<'_>);

    fn on_input(&mut self, events: &[InputEvent]);

    fn update(&mut self, ctx: &mut RenderBackendUpdateCtx);

    fn render(&mut self, ctx: &RenderBackendRenderCtx);

    fn camera(&self) -> &Camera;

    fn on_resize(&mut self, _ctx: RenderAppResizeCtx<'_>) {}

    fn shutdown(&mut self) {}
}
