//! Assimp C++ 互操作层
//!
//! 本 crate 通过 bindgen 生成 `truvixx-assimp-capi` 的 Rust FFI 声明，并向
//! Cargo 声明链接已由 `truvis-cxx-build` 复制到 Cargo 输出目录的
//! `truvixx-assimp-capi`。CMake 构建、Debug/Release 产物复制和运行时 DLL
//! 布置属于 `truvis-cxx-build` 的职责。

pub mod _ffi_bindings {
    include!(env!("TRUVIS_ASSIMP_BINDINGS_RS"));
}

// 生成文件路径保持稳定；内容 hash 只用于让 Cargo / rust-analyzer 感知 binding 内容变化。
const _: &str = env!("TRUVIS_ASSIMP_BINDINGS_HASH");
const _: &str = include_str!(env!("TRUVIS_ASSIMP_BINDINGS_HASH_FILE"));

pub use crate::_ffi_bindings::root as truvixx;
