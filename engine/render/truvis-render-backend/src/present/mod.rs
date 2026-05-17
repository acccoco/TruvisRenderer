//! swapchain/present 生命周期封装。
//!
//! 该模块拥有 surface、swapchain wrapper、swapchain image/view handle 与 acquire/present
//! 同步对象。render pass 只通过 `RenderPresent` 暴露的 handle/semaphore 接入窗口图像，
//! 不直接持有或销毁 WSI 对象。

pub mod render_present;
