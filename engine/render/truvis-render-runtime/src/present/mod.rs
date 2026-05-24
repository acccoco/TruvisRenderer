//! swapchain/present 生命周期封装。
//!
//! 该模块拥有 surface、swapchain wrapper、swapchain image/view handle 与 acquire/present
//! 同步对象。app/plugin 只通过 `PresentView` 查询 present 信息，并通过
//! `ImportedPresentTarget` 接入 RenderGraph，不直接持有 swapchain owner、image wrapper
//! 或 semaphore。

pub mod swapchain_presenter;
