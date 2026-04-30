## Why

`RenderContext` 是一个持有全部渲染状态的单一大袋子，CPU 场景数据（`SceneManager`、`AssetHub`）和 GPU 渲染状态（`GpuScene`、`BindlessManager`、`GlobalDescriptorSets`、`FifBuffers`）混在同一个所有权域中。这导致：

- 所有 AppPlugin contexts（`InitCtx`、`UpdateCtx`、`RenderCtx`、`ResizeCtx`）都从同一个结构体中借出切片，无法在类型层面体现 phase 边界
- Render passes 拿到整个 `&RenderContext` 却只用 GPU 侧字段（每个 pass 都标注了 `// TODO 暂时使用这个肮脏的实现`）
- `truvis-render-passes` 仅为获取 `RenderContext` 类型就必须依赖 `truvis-renderer`（L4），导致本应在 L3 的 pass 实现被拉到 L5
- CPU 状态和 GPU 状态无法独立演化，阻塞后续 extract phase 显式化和多 plugin 支持

四次架构重构（依赖清理 → 渲染线程隔离 → FrameRuntime 边界 → API crate 拆分）已将系统推进到 FrameRuntime + AppPlugin 模型。现在是拆分 RenderContext、建立 World / RenderWorld 物理分离的合适时机。

## What Changes

- **新增 `RenderWorld` 结构体**：定义在 `truvis-render-interface`，持有全部 GPU 侧资源（`GpuScene`、`BindlessManager`、`GlobalDescriptorSets`、`GfxResourceManager`、`FifBuffers`、`SamplerManager`、`PerFrameDataBuffers`）和帧状态（`FrameCounter`、`FrameSettings`、`PipelineSettings`、`AccumData`、时间值）
- **新增 `truvis-world` crate**：定义 `World` 结构体，持有 CPU 侧状态（`SceneManager`、`AssetHub`）
- **`FifBuffers` 从 `truvis-render-graph` 迁移到 `truvis-render-interface`**：FifBuffers 仅依赖 render-interface 和 gfx 的类型，与 render graph 概念无关，属于放错位置的类型
- **`Renderer` 改为持有 `World` + `RenderWorld`**，**BREAKING**: 删除 `RenderContext` 和 `RenderContext2`
- **BREAKING**: `AppPlugin` 的 phase contexts 改用 `World` / `RenderWorld`：`RenderCtx` 持有 `&RenderWorld`（替代 `&RenderContext`）；`UpdateCtx` 持有 `&mut World`；`InitCtx` 持有 `&mut World` + `&mut RenderWorld`；`ResizeCtx` 持有 `&mut RenderWorld`
- **BREAKING**: 全部 render pass（`SdrRgPass`、`BlitRgPass`、`ResolveRgPass`、`RealtimeRtRgPass`、`DenoiseAccumRgPass`、`AccumRgPass`、`GuiRgPass`）的 `render_context` 字段改为 `render_world`
- **`truvis-render-passes` 去掉对 `truvis-renderer` 的依赖**：pass 改为引用 `render-interface` 中的 `RenderWorld`，从 L5 降到 L3

## Capabilities

### New Capabilities
- `world-renderworld-split`: 将 RenderContext 拆分为 World（CPU 状态）和 RenderWorld（GPU 状态），建立物理分离的所有权边界

### Modified Capabilities
- `render-context-split`: RenderContext 被删除，其职责由 World + RenderWorld 接管，原 spec 中"RenderContext 定义在 truvis-renderer"的要求不再适用

## Impact

- **直接修改的 crate**: `truvis-render-interface`（新增 RenderWorld + 迁入 FifBuffers）、`truvis-renderer`（Renderer 重构 + 删除 RenderContext）、`truvis-app-api`（context 类型更新）、`truvis-frame-runtime`（context 构造更新）、`truvis-render-passes`（7 个 pass 更新 + 去掉 renderer 依赖）、`truvis-app`（demo apps + GuiRgPass 更新）、`truvis-render-graph`（移出 FifBuffers）
- **新增 crate**: `truvis-world`（World 定义）
- **删除的类型**: `RenderContext`、`RenderContext2`
- **分层变化**: `truvis-render-passes` 从 L5 降到 L3；`truvis-world` 位于 L3（与 scene/asset 同层）
- **所有 demo app 需要适配**: 构造 RenderCtx 和 pass 时的字段名变化（`render_context` → `render_world`）
