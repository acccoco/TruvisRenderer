# truvis-gfx

Vulkan RHI 抽象层，封装设备、队列、资源、同步与图形/计算/光追管线基础能力。

## 主要内容

- `Gfx` 显式 root owner 与 typed Gfx Ctx 工厂
- 命令缓冲、同步原语、屏障封装
- 图像/缓冲等 GPU 资源对象
- 交换链与渲染目标底层支持

## Ctx 规则

- `Gfx::new(...)` 返回由调用方持有的 root owner；生产代码不通过全局 singleton 访问 `Gfx`。
- `GfxDeviceCtx`、`GfxResourceCtx`、`GfxQueueCtx`、`GfxSurfaceCtx`、`GfxDeviceInfoCtx`、`GfxImmediateCtx` 只暴露当前能力需要的 Vulkan 依赖。
- 上层创建、提交、查询或销毁资源时，应传入最窄的 typed Ctx，避免把完整 `&Gfx` 或长期引用存进资源对象。

## 生命周期规则

- Vulkan/VMA/WSI wrapper 只通过显式 `destroy(...ctx...)` / `destroy_mut(...ctx...)` 释放底层对象。
- `Drop` 只做 debug assertion，用于暴露遗漏的显式销毁，不再调用 Vulkan/VMA/WSI release API。
- `GfxBuffer`、`GfxImage`、`GfxImageView`、swapchain/surface、pipeline、descriptor、sampler、query、sync 等对象的销毁依赖必须由 owner 在调用点传入。
- `GfxSemaphore` 是唯一 owner，不可 clone；提交代码应传引用或 raw handle。
- VMA-backed buffer/image 创建使用 `resources::vma_debug::with_vma_debug_name` 写入 copied allocation user data。调用点不应直接设置裸 `user_data` 指针。

## 依赖关系

- 上层几乎所有渲染模块都依赖本 crate
- 本 crate 不应依赖更高层业务语义模块
