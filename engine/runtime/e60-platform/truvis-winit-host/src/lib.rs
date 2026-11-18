//! winit standalone 顶层窗口与 Windows embedded child window 宿主。
//!
//! 本 crate 只拥有窗口、事件循环和输入适配；OS RenderThread 生命周期由
//! `truvis-render-thread` 统一管理，具体产品策略由 app 入口注入。

pub mod embedded;
pub mod standalone;

mod input_adapter;
mod win32;

pub use embedded::{EmbeddedViewportRect, EmbeddedWinitHost};
pub use standalone::{StandaloneWindowOptions, StandaloneWinitHost};
