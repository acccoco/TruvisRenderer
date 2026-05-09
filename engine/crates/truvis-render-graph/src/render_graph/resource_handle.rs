//! RenderGraph 资源句柄定义
//!
//! 使用 slotmap 的 `new_key_type!` 宏定义类型安全的资源句柄。
//! 这些句柄是 graph 内部的虚拟引用，与 `GfxResourceManager` 的物理句柄分离。

use slotmap::new_key_type;

new_key_type! {
    /// Graph 内部的 Image 句柄
    ///
    /// 用于在 RenderGraph 构建阶段引用图像资源。
    /// 基于 slotmap 实现，自带版本验证机制。
    pub struct RgImageHandle;
}
