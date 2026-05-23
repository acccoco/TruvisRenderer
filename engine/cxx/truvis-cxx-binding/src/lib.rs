//! C++ 互操作层
//!
//! 本 crate 通过 bindgen 生成 `truvixx-interface` 的 Rust FFI 声明，并向 Cargo
//! 声明链接已由 `truvis-cxx-build` 复制到 target 目录的 `truvixx-interface`。
//! CMake 构建、Debug/Release 产物复制和运行时 DLL 布置属于 `truvis-cxx-build` 的职责。

pub mod _ffi_bindings;
pub use crate::_ffi_bindings::root as truvixx;
