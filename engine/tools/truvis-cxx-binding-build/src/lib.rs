//! CXX binding 的生成、native 链接与运行文件复制支持。
//!
//! binding owner 在 build.rs 中声明生成参数和文件列表，本 crate 复用生成与文件消费逻辑。
//! Native module 不依赖本 crate；CMake 由外层先完成构建。

mod codegen;
mod native_library;

pub use codegen::{CxxBindingGenerator, CxxBindingSpec, GeneratedCxxBinding, ModuleRawLine};
pub use native_library::NativeLibrary;
