//! Renderer / RenderLoop 框架层。
//!
//! 本 crate 位于 render-loop 层，集中定义 `RenderLoop` 和具体 Renderer 之间如何交接
//! 生命周期与阶段上下文。`RenderLoop` 合并固定帧骨架与渲染线程主循环，但不创建平台窗口，
//! 也不感知具体 Renderer 如何静态组合 GUI、相机、overlay 或渲染子系统。
//!
//! 主要抽象：
//! - [`RenderLoop`]：唯一完整帧执行器，统一拥有 Renderer、Runtime 和 render loop。
//! - [`Renderer`](renderer::Renderer)：`RenderLoop` 内部持有的顶层渲染业务契约。
//! - [`InputEvent`](input_event::InputEvent)：平台输入事件转换后的引擎侧表示。
//! - [`RenderThreadControl`] / [`RenderThreadInit`]：平台与 `RenderLoop` 之间的线程控制契约。
//!
//! 这里的上下文类型只把当前阶段需要的 `RenderRuntime` 能力裁剪出来传给调用者。
//! 调用方不应从这些上下文中长期保存 typed `Gfx` ctx 或 runtime 内部引用。

pub mod input_event;
pub mod renderer;

mod render_loop;
mod render_thread_control;

pub use render_loop::RenderLoop;
pub use render_thread_control::{RenderThreadControl, RenderThreadInit};
pub use renderer::{Renderer, RendererInitCtx, RendererResizeCtx};

pub fn init_env() {
    init_env_with_log_init(truvis_logs::TruvisLogger::init);
}

pub fn init_env_with_log_file(log_file_path: impl AsRef<std::path::Path>) {
    init_env_with_log_init(move || truvis_logs::TruvisLogger::init_with_file(log_file_path));
}

fn init_env_with_log_init(init_log: impl FnOnce()) {
    std::panic::set_hook(Box::new(|info| {
        log::error!("{}", info);
    }));
    init_log();
    tracy_client::Client::start();
}
