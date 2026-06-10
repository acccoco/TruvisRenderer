# truvis-gfx

Vulkan RHI 抽象层，封装设备、队列、资源、同步与图形/计算/光追管线基础能力。

## 主要内容

- `Gfx` 显式 root owner 与 typed Gfx Ctx 工厂
- 命令缓冲、同步原语、屏障封装
- 图像/缓冲等 GPU 资源对象
- `GfxSBTBuffer` 负责 ray tracing SBT buffer、region 对齐和 shader group handle 写入
- 交换链与渲染目标底层支持

## Root owner

- `Gfx` 是 Vulkan root owner，由上层显式创建并持有；生产代码不通过全局 singleton 访问 GPU 设备。
- 默认 `Gfx::new(...)` 同时拥有 Streamline runtime 生命周期：先执行 `slInit`，再通过
  `sl.interposer.dll` 创建 Vulkan entry，最后在 `Gfx::destroy(...)` 释放内部 device child 后、
  销毁 Vulkan device/instance 前执行 `slShutdown`。
- `Gfx::new_with_entry_source(...)` 是测试或特殊启动路径使用的显式 Vulkan loader 注入入口；
  它只按传入的 `VulkanEntrySource` 创建 Vulkan root，不自动初始化或持有 Streamline runtime。
- `Gfx` 负责创建 typed Ctx，并在所有 child resources 显式销毁后释放内部 command pool /
  allocator、关闭 Streamline，再销毁 device、instance、surface 相关 root 资源。
- 上层 owner 负责安排销毁顺序，`Gfx` 不隐藏清理仍被外部持有的资源。

## Ctx 规则

- `Gfx::new(...)` 返回由调用方持有的 root owner；生产代码不通过全局 singleton 访问 `Gfx`。
- `GfxDeviceCtx`、`GfxResourceCtx`、`GfxQueueCtx`、`GfxSurfaceCtx`、`GfxDeviceInfoCtx`、`GfxImmediateCtx` 只暴露当前能力需要的 Vulkan 依赖。
- 上层创建、提交、查询或销毁资源时，应传入最窄的 typed Ctx，避免把完整 `&Gfx` 或长期引用存进资源对象。

## 生命周期规则

- Vulkan/VMA/WSI wrapper 只通过显式 `destroy(...ctx...)` / `destroy_mut(...ctx...)` 释放底层对象。
- `Drop` 只做 debug assertion，用于暴露遗漏的显式销毁，不再调用 Vulkan/VMA/WSI release API。
- Streamline 由 `Gfx` 显式包住 Vulkan root 生命周期；调用方不应在 `RenderRuntime` 或更上层指定
  Streamline plugin 路径、日志路径或 Vulkan loader 路径，也不应直接决定 SL 开关。
- `GfxBuffer`、`GfxImage`、`GfxImageView`、swapchain/surface、pipeline、descriptor、sampler、query、sync 等对象的销毁依赖必须由 owner 在调用点传入。
- swapchain 创建前必须查询 surface 支持的 formats 与 present modes；项目默认格式和 present mode 是硬性要求，不支持时启动阶段直接失败。
- `GfxSemaphore` 是唯一 owner，不可 clone；提交代码应传引用或 raw handle。
- VMA-backed buffer/image 创建使用 `resources::vma_debug::with_vma_debug_name` 写入 copied allocation user data。调用点不应直接设置裸 `user_data` 指针。

## 设计意图

- typed Ctx 把 Vulkan 能力按阶段拆窄：resource allocation、descriptor/pipeline、queue submit、surface/swapchain、device info 和 immediate helper 分别使用对应 context。
- 长期资源对象不保存 `&Gfx`、`&GfxDevice` 或 allocator 引用，销毁依赖由 owner 在调用点传入。
- `Drop` 只做遗漏销毁诊断，避免在不可控 drop 顺序中调用 Vulkan/VMA/WSI release API。

## 依赖关系

- 上层几乎所有渲染模块都依赖本 crate
- 本 crate 不应依赖更高层业务语义模块
