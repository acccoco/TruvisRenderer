# truvis-render-interface

渲染契约层，提供帧调度、资源句柄、全局描述符与 GPU 资源管理通用原语。

## 关键组件

- `FrameCounter` / `FrameLabel`
- `CmdAllocator`
- `GfxResourceManager`（Handle + 生命周期管理）
- `BindlessManager` / `GlobalDescriptorSets`

## 资源管理规则

- `GfxResourceManager` 是 manager-owned image/view 的唯一释放入口，负责 view 先于 image 销毁。
- 延迟销毁通过 frame id 入队，`cleanup()` 到达安全帧后释放，并记录 `DestroyReason::DeferredCleanup`。
- resize / shutdown / immediate release 必须使用带 `DestroyReason` 的 release API，便于把日志关联到项目资源名、raw Vulkan handle 与 manager handle。
- `FifBuffers` 只保存 manager handle；resize 和 shutdown 时先注销 bindless，再通过 `GfxResourceManager` 释放 image，view 随 image 释放。
- `CmdAllocator::destroy()` 是显式 shutdown 边界；`Drop` 只断言它已经被销毁。

## 模块定位

- 位于 RHI 与上层渲染逻辑之间
- 提供稳定的数据契约，减少上层直接触碰底层细节
