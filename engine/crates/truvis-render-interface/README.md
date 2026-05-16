# truvis-render-interface

渲染契约层，提供帧调度、资源句柄、全局描述符与 GPU 资源管理通用原语。

## 关键组件

- `FrameCounter` / `FrameLabel`
- `CmdAllocator`
- `GfxResourceManager`（Handle + 生命周期管理）
- `BindlessManager` / `GlobalDescriptorSets`
- `RenderWorld`

## RenderWorld

- `RenderWorld` 是 GPU 侧运行时状态集合，包含 `GpuScene`、`BindlessManager`、`GlobalDescriptorSets`、`GfxResourceManager`、`FifBuffers`、sampler manager、per-frame data、frame counter 和 frame/pipeline settings。
- `RenderWorld` 不包含 CPU scene 或 asset hub；这些数据属于 `truvis-world::World`。
- render 阶段通常只借出 `&RenderWorld`，使 pass adapter 能读取 GPU 状态并录制命令，但不能随意改写 frame state。
- resize / shutdown 阶段通过 mutable context 暴露 `RenderWorld`，用于重建或释放 manager-owned GPU resources。

## 资源管理规则

- 本层 API 通过 typed `Gfx` Ctx 接收底层能力；`RenderWorld` 和长期资源字段不保存 Ctx 引用。
- `GfxResourceManager` 是 manager-owned image/view 的唯一释放入口，负责 view 先于 image 销毁。
- 延迟销毁通过 frame id 入队，`cleanup()` 到达安全帧后释放，并记录 `DestroyReason::DeferredCleanup`。
- resize / shutdown / immediate release 必须使用带 `DestroyReason` 的 release API，便于把日志关联到项目资源名、raw Vulkan handle 与 manager handle。
- `FifBuffers` 只保存 manager handle；resize 和 shutdown 时先注销 bindless，再通过 `GfxResourceManager` 释放 image，view 随 image 释放。
- `CmdAllocator`、`GlobalDescriptorSets`、`RenderSamplerManager`、`GpuScene` 等 owner 在 shutdown 时接收 phase Ctx 并显式销毁自身持有的 GPU 资源。
- `Drop` 只保留诊断职责，不作为 Vulkan/VMA 释放路径。

## 模块定位

- 位于 RHI 与上层渲染逻辑之间
- 提供稳定的数据契约，减少上层直接触碰底层细节
- 不依赖 App、Plugin、scene loading 或窗口平台语义
