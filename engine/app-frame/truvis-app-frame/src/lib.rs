//! App 框架层。
//!
//! 本 crate 位于 frame 层，集中定义 `RenderAppRunner`、具体 App 和 Plugin
//! 之间如何交接生命周期与阶段上下文。Runner 合并固定帧骨架与渲染线程主循环，
//! 但不创建平台窗口，也不决定具体 App 如何组合 GUI、相机、overlay 或 render pipeline plugin。
//!
//! 主要抽象：
//! - [`RenderAppRunner`]：唯一完整帧执行器，统一拥有 App、Runtime 和 render loop。
//! - [`RenderApp`](render_app_api::RenderApp)：Runner 内部持有的具体 App 契约。
//! - [`Plugin`](plugin_api::Plugin)：App 持有的可复用能力单元的标准生命周期契约。
//! - [`InputEvent`](input_event::InputEvent)：平台输入事件转换后的引擎侧表示。
//! - [`RenderThreadControl`] / [`RenderThreadInit`]：平台与 Runner 之间的线程控制契约。
//!
//! 这里的上下文类型只把当前阶段需要的 `RenderRuntime` 能力裁剪出来传给调用者。
//! 调用方不应从这些上下文中长期保存 typed `Gfx` ctx 或 runtime 内部引用。

pub mod input_event;
pub mod plugin_api;
pub mod render_app_api;

mod render_app_runner;
mod render_thread_control;

pub use render_app_api::{RenderApp, RenderAppInitCtx, RenderAppResizeCtx};
pub use render_app_runner::RenderAppRunner;
pub use render_thread_control::{RenderThreadControl, RenderThreadInit};

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
