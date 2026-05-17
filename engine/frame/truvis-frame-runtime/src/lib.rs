//! 基础帧编排运行时。
//!
//! [`RenderAppShell`] 驱动固定的每帧生命周期：
//! `begin_frame` → 输入 hook → update hook → prepare → render hook → present → `end_frame`。
//!
//! 具体 app hooks 持有 GUI、camera、input state、overlay 和 render plugin。

mod render_app_shell;

pub use render_app_shell::RenderAppShell;
pub use truvis_frame_api::render_app::{RenderAppHooks, RenderAppInitCtx, RenderAppResizeCtx};

pub fn init_env() {
    std::panic::set_hook(Box::new(|info| {
        log::error!("{}", info);
    }));
    truvis_logs::init_log();
    tracy_client::Client::start();
}
