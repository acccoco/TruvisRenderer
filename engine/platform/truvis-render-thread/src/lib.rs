//! 独立于窗口 backend 的 OS RenderThread 生命周期宿主。
//!
//! 本 crate 只负责编排线程、Runner 与窗口 owner 之间的控制和完成握手；
//! 平台窗口句柄的提取、跨线程合法性和窗口存活时间由具体窗口宿主负责。

mod render_thread;

pub use render_thread::{RenderAppFactory, RenderThread};
