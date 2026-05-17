//! swapchain/present 生命周期封装。
//!
//! 该模块拥有 surface、swapchain wrapper、swapchain image/view handle 与 acquire/present
//! 同步对象。app/plugin 只通过 `PresentView` / `PresentTargetView` 接入窗口图像，
//! 不直接持有 swapchain owner 或销毁 WSI 对象。

pub mod render_present;
