//! C++ 互操作层
//!
//! 通过 CMake + bindgen 集成 C++ 库（如 Assimp），提供 Rust 安全封装。
//! `build.rs` 自动处理 CMake 构建和 DLL 复制。

pub mod _ffi_bindings;
pub use crate::_ffi_bindings::root as truvixx;
