//! runtime 与 app crate 共享的 frame app 和 plugin 契约。
//!
//! 本 crate 只定义契约和类型化上下文：
//! - [`FrameApp`](frame_app::FrameApp) — object-safe render-loop app 契约
//! - [`FrameAppHooks`](frame_app::FrameAppHooks) — `BaseApp` 使用的 hook 点
//! - [`Plugin`](plugin::Plugin) — 可复用能力的生命周期契约
//! - [`InputEvent`](input_event::InputEvent) — 平台输入事件类型

pub mod frame_app;
pub mod input_event;
pub mod plugin;
