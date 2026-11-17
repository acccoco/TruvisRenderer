//! Streamline / DLSS C++ 互操作层。
//!
//! 本 crate 只面向 Windows x64。它不做跨平台抽象，也不隐藏 Windows 路径编码细节：
//! Streamline SDK 的接口需要 UTF-16 路径，Vulkan loader 也会以 Windows DLL 的形式
//! 从 executable 所在目录加载。
//!
//! 本 crate 覆盖 Streamline runtime 与 DLSS SR/RR 的 native 调用边界：
//! - C++ 持有进程级 DLL/函数表与初始化状态，转换 SDK 类型和日志枚举。
//! - Rust 侧持有进程级 RAII 句柄，管理初始化状态、日志桥生命周期和 shutdown 时序。
//! - DLSS 参数、tag/evaluate/free 的 ABI 封装见 dlss；GPU 对象与 RenderGraph 顺序仍由调用方负责。
//! - SDK 初始化/关闭失败后不支持恢复；native 保留模块引用，不能据此声称 SDK 资源已经回收。
//!
//! 重要生命周期约定：启用 DLSS 时，应先初始化 [`StreamlineRuntime`]，再用
//! [`StreamlineRuntime::vulkan_loader_path`] 创建 `truvis-gfx` 的 Streamline Vulkan loader。
//! 关闭时调用方应先释放上层 GPU child resources，再在 Vulkan device / instance 等 root
//! 对象销毁前 drop [`StreamlineRuntime`]，确保 `slShutdown` 仍处在有效 Vulkan root 生命周期内。

pub mod _ffi_bindings {
    include!(env!("TRUVIS_STREAMLINE_BINDINGS_RS"));
}

// 生成文件路径保持稳定；内容 hash 只用于让 Cargo / rust-analyzer 感知 binding 内容变化。
const _: &str = env!("TRUVIS_STREAMLINE_BINDINGS_HASH");
const _: &str = include_str!(env!("TRUVIS_STREAMLINE_BINDINGS_HASH_FILE"));

pub use crate::_ffi_bindings::root as truvixx;

mod config;
pub mod dlss;
mod log_bridge;
mod runtime;

pub use config::{StreamlineFeatureFlags, StreamlineInitInfo};
pub use runtime::{StreamlineError, StreamlineRuntime};
