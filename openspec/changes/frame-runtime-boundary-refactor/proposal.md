## Why

`render-thread-isolation` 完成后，渲染循环已经从 winit 主线程剥离，但应用主框架仍存在明显的职责重叠：

- `RenderApp` 既做帧编排，也承载默认业务行为（overlay UI、输入处理细节）
- `Renderer` 既做 GPU backend，也直接触发 scene/asset 相关更新
- `OuterApp` 同时承担场景数据提供、渲染策略、UI 构建、平台事件响应等多重角色
- `truvis-app` 同时包含框架层、示例应用、具体 pass 实现，crate 边界不清晰

这使得命名语义和实际职责不一致，增加重构成本，并阻碍后续的 crate 拆分与模块复用。

## What Changes

- 引入命名与职责收敛：
  - `RenderApp` 重命名为 `FrameRuntime`（过渡期可保留兼容别名）
  - `OuterApp` 升级为 `AppPlugin`（单 trait，多阶段 hook）
  - `Renderer` 语义收敛为 render backend（本 change 不强制类型改名）
- `FrameRuntime` 将帧流程显式拆分为阶段函数（input/update/prepare/render/present），替代单体 `big_update` 叙事。
- `Renderer` 移除 scene/asset 侧的主动更新职责，仅保留 GPU 执行与提交相关能力。
- 默认 overlay UI 从 runtime 硬编码路径中剥离，改为可注册模块。
- 引入 `LegacyOuterAppAdapter` 兼容层，保证现有 demo 平滑迁移，不做一次性破坏性切换。
- `render_pipeline/*` 先完成逻辑解耦，物理迁移到 `truvis-render-passes` 作为后续 change。

## Capabilities

### New Capabilities

- `frame-runtime-boundary`: 定义主框架中的 runtime/backend/plugin 边界、命名语义、阶段编排与兼容迁移策略。

### Modified Capabilities

- `render-threading`: 在保持线程隔离不变的前提下，渲染线程内部的帧推进入口从旧 `RenderApp` 语义过渡到 `FrameRuntime` 语义。

## Impact

- `engine/crates/truvis-app/src/render_app.rs`：重命名为 runtime 语义模块，并拆分阶段函数。
- `engine/crates/truvis-app/src/outer_app/*`：升级为 plugin 语义与兼容适配层。
- `engine/crates/truvis-renderer/src/renderer.rs`：剥离非 backend 职责，收敛 API。
- `engine/crates/truvis-app/src/gui_front.rs` 及相关 UI 路径：默认 overlay 改为可注册模块。
- `truvis-winit-app`：保持线程与平台职责不变，仅更新对 runtime 新命名/接口的调用对接。
- 后续可选影响（非本 change 实施）：新增 `truvis-render-passes` crate，并将通用 pass 从 `truvis-app` 迁出。
