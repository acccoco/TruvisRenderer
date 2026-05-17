//! runtime 与 app crate 共享的 render app 和 plugin 契约。
//!
//! 本 crate 只定义契约和类型化上下文：
//! - [`RenderApp`](render_app::RenderApp) — object-safe render-loop app 契约
//! - [`RenderAppHooks`](render_app::RenderAppHooks) — `RenderAppShell` 使用的 hook 点
//! - [`Plugin`](plugin::Plugin) — 可复用能力的生命周期契约
//! - [`InputEvent`](input_event::InputEvent) — 平台输入事件类型

pub mod input_event;
pub mod plugin;
pub mod render_app;
