//! App 框架层。
//!
//! 本 crate 位于 frame 层，集中定义 render loop、`RenderAppShell`、具体 App
//! 和 Plugin 之间如何交接生命周期与阶段上下文。它持有 App 框架需要的
//! `RenderRuntime` shell 与渲染线程主循环，但不创建平台窗口，也不决定具体 App
//! 如何组合 GUI、相机、overlay 或 render pipeline plugin。
//!
//! 主要抽象：
//! - [`RenderAppShell`]：render loop 直接驱动的唯一完整帧骨架。
//! - [`RenderApp`](render_app_api::RenderApp)：Shell 内部持有的具体 App 契约。
//! - [`Plugin`](plugin_api::Plugin)：App 持有的可复用能力单元的标准生命周期契约。
//! - [`InputEvent`](input_event::InputEvent)：平台输入事件转换后的引擎侧表示。
//! - [`render_loop`](render_loop::render_loop)：渲染线程入口，只驱动 [`RenderAppShell`]。
//!
//! 这里的上下文类型只把当前阶段需要的 `RenderRuntime` 能力裁剪出来传给调用者。
//! 调用方不应从这些上下文中长期保存 typed `Gfx` ctx 或 runtime 内部引用。

pub mod input_event;
pub mod plugin_api;
pub mod render_app_api;
pub mod render_loop;
pub mod render_thread;

mod render_app_shell;

pub use render_app_api::{RenderApp, RenderAppInitCtx, RenderAppResizeCtx};
pub use render_app_shell::RenderAppShell;
pub use render_loop::render_loop;
pub use render_thread::{RenderInitMsg, SendWrapper, SharedState, pack_size, unpack_size};

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
