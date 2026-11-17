//! 复用 ash loader 和 Vulkan 对象的 native 命令 binding。
//!
//! 本 crate 仅持有设备函数表，不加载 Vulkan DLL、不创建或销毁 Vulkan 对象。
//! 所有命令同步转发到 C ABI；原始句柄和 descriptor 内存的有效期由调用方保证。

mod device;

#[allow(warnings)]
mod bindings {
    include!(env!("TRUVIS_VK_BINDINGS_RS"));
}

use bindings::root as ffi;

const _: &str = env!("TRUVIS_VK_BINDINGS_HASH");
const _: &str = include_str!(env!("TRUVIS_VK_BINDINGS_HASH_FILE"));

pub use device::{Device, LoadError};
