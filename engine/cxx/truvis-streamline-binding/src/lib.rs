//! Streamline / DLSS C++ 互操作层。
//!
//! 本 crate 只面向 Windows x64。它不做跨平台抽象，也不隐藏 Windows 路径编码细节：
//! Streamline SDK 的接口需要 UTF-16 路径，Vulkan loader 也会以 Windows DLL 的形式
//! 从 executable 所在目录加载。
//!
//! 当前阶段只覆盖 Streamline runtime 生命周期：
//! - C++ 侧是零状态的纯 ABI 转发层，只负责 `sl::Preferences` 组装和 `sl::LogType` 枚举转换。
//! - Rust 侧持有进程级 RAII 句柄，管理初始化状态、日志桥生命周期和 shutdown 时序。
//! - Vulkan object、RenderGraph pass、resource tagging、`slEvaluateFeature` 都属于后续阶段。
//!
//! 重要生命周期约定：启用 DLSS 时，应先初始化 [`StreamlineRuntime`]，再用
//! [`StreamlineRuntime::vulkan_loader_path`] 创建 `truvis-gfx` 的 Streamline Vulkan loader。
//! 关闭时调用方应先释放上层 GPU child resources，再在 Vulkan device / instance 等 root
//! 对象销毁前 drop [`StreamlineRuntime`]，确保 `slShutdown` 仍处在有效 Vulkan root 生命周期内。

pub mod _ffi_bindings;
pub use crate::_ffi_bindings::root as truvixx;

mod config;
pub mod dlss;
mod log_bridge;
mod runtime;

pub use config::{StreamlineFeatureFlags, StreamlineInitInfo};
pub use runtime::{StreamlineError, StreamlineRuntime};
