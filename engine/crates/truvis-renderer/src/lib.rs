//! 渲染管线层
//!
//! 提供高层渲染抽象，包括 [`FrameContext`] 单例、渲染管线、GPU 场景管理等。
//! 通过 [`FrameContext`] 统一管理帧资源、命令分配器、Bindless 描述符等核心子系统。

pub mod platform;
pub mod present;
pub mod subsystems;

pub mod model_loader;
pub mod renderer;
