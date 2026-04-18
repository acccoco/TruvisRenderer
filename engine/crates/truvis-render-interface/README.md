# truvis-render-interface

渲染契约层，提供帧调度、资源句柄、全局描述符与 GPU 资源管理通用原语。

## 关键组件

- `FrameCounter` / `FrameLabel`
- `CmdAllocator`
- `GfxResourceManager`（Handle + 生命周期管理）
- `BindlessManager` / `GlobalDescriptorSets`

## 模块定位

- 位于 RHI 与上层渲染逻辑之间
- 提供稳定的数据契约，减少上层直接触碰底层细节
