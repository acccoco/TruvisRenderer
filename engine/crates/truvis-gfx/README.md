# truvis-gfx

Vulkan RHI 抽象层，封装设备、队列、资源、同步与图形/计算/光追管线基础能力。

## 主要内容

- `Gfx` 全局上下文访问
- 命令缓冲、同步原语、屏障封装
- 图像/缓冲等 GPU 资源对象
- 交换链与渲染目标底层支持

## 生命周期规则

- RAII-owned wrapper 在 `Drop` 中释放底层 Vulkan/VMA 对象；公开的 `destroy(self)` 只表示“立刻 drop”。
- `GfxBuffer` 与 special buffer wrapper 属于 RAII-owned。它们的 `Drop` 依赖 `Gfx::get()`，因此必须在 `Gfx::destroy()` 之前离开作用域或被显式 drop。
- `GfxImage` / `GfxImageView` 属于 manager-owned 或 lifecycle-owner-owned；它们只通过显式 `destroy(reason)` 释放，`Drop` 只做 debug assertion。
- `GfxSemaphore` 是唯一 owner，不可 clone；提交代码应传引用或 raw handle。
- VMA-backed buffer/image 创建使用 `resources::vma_debug::with_vma_debug_name` 写入 copied allocation user data。调用点不应直接设置裸 `user_data` 指针。

## 依赖关系

- 上层几乎所有渲染模块都依赖本 crate
- 本 crate 不应依赖更高层业务语义模块
