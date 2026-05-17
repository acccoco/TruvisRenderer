//! runtime、平台入口和 app crate 共享的帧级契约。
//!
//! 本 crate 位于 frame 层的 API 边界，只描述 render loop、`RenderAppShell`、
//! 具体 App hooks 和 Plugin 之间如何交接生命周期与阶段上下文。它不持有
//! `RenderBackend` 状态，不创建窗口，不实现帧循环，也不决定具体 App 如何组合
//! GUI、相机、overlay 或 render pipeline plugin。
//!
//! 主要抽象：
//! - [`RenderApp`](render_app::RenderApp)：平台 render loop 能以 trait object
//!   驱动的外部 App 契约。
//! - [`RenderAppHooks`](render_app::RenderAppHooks)：`RenderAppShell` 回调具体
//!   App 的固定 hook 点。
//! - [`Plugin`](plugin::Plugin)：App 持有的可复用能力单元的标准生命周期契约。
//! - [`InputEvent`](input_event::InputEvent)：平台输入事件转换后的引擎侧表示。
//!
//! 这里的上下文类型只把当前阶段需要的 `RenderBackend` 能力裁剪出来传给调用者。
//! 调用方不应从这些上下文中长期保存 typed `Gfx` ctx 或 backend 内部引用。

pub mod input_event;
pub mod plugin;
pub mod render_app;
