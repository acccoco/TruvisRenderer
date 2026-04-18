## Context

当前工程在类型命名与职责边界上存在系统性错位：

- `RenderApp` 的名字指向“应用”，但实际承担的是“帧阶段编排器”
- `Renderer` 的名字指向“渲染后端”，但实际混入了部分 world/update 职责
- `OuterApp` 的名字语义模糊，且把 scene/update/render/ui/platform hook 混在一个对象里
- `truvis-app` crate 同时承载框架层、示例层和 pass 实现层

`render-thread-isolation` 已经把平台线程问题解决，本 change 聚焦于“语义边界与演进路径”，不触及线程模型本身。

## Goals / Non-Goals

**Goals**

- 统一主框架命名语义，使类型名与职责一致
- 显式化帧阶段（phase）边界，降低 `big_update` 复杂度
- 收敛 `Renderer` 到 backend 能力边界
- 提供平滑迁移路径，避免一次性破坏现有 demo
- 为后续 crate 物理拆分提供稳定接口基础

**Non-Goals**

- 不在本 change 完成 `Gfx::get()` 全量去单例化
- 不在本 change 一次性物理拆 crate
- 不引入完整 Main World / Render World 双 world 架构
- 不变更 `render-thread-isolation` 的线程协议与关闭握手

## Decisions

### 决策 1：命名收敛

- `RenderApp` -> `FrameRuntime`
- `OuterApp` -> `AppPlugin`（保留兼容 adapter）
- `Renderer` 暂不强制改名，但 API 语义按 backend 收敛

原因：先统一语义，再做文件与 crate 迁移，可降低改动面和回滚复杂度。

### 决策 2：`AppPlugin` 采用“单 trait 多 hook”

在过渡阶段保留单 trait，按帧阶段暴露 hook（如 update/build_ui/extract/render），不立即拆成 `SceneProvider` + `RenderSubsystem` 两个 trait。

原因：兼容成本最低，先把行为边界做清，再决定是否做双 trait 的长期抽象。

### 决策 3：`FrameRuntime` 持有编排职责，不持有硬编码业务

`FrameRuntime` 负责阶段顺序与上下文装配，但默认 overlay UI 改为可注册模块，不再硬编码在 runtime 主流程中。

### 决策 4：`Renderer` 只保留 backend 职责

`Renderer` 负责 device/swapchain/cmd/sync/submit/present 等 GPU 路径，不再主动推进 scene/asset 的 world 更新逻辑。

这些更新逻辑由 `FrameRuntime` 的 phase 驱动。

### 决策 5：兼容迁移优先于一次性切换

引入 `LegacyOuterAppAdapter`，确保 `triangle`、`rt-cornell`、`rt-sponza`、`shader-toy` 逐步迁移。

兼容窗口为 1-2 个 change 周期，迁移完成后移除旧接口。

### 决策 6：crate 策略分两步

- Step A（本 change）：先在现有 crate 内完成职责与接口重构
- Step B（后续 change）：再做物理拆分（`truvis-frame-runtime` / `truvis-app-api` / `truvis-render-passes`）

原因：避免“接口变化 + crate 搬迁”同时发生造成高风险回归。

## Target Architecture (Transition State)

```text
truvis-winit-app
  └─ Winit host (window/event/thread lifecycle)

truvis-app
  ├─ FrameRuntime (phase orchestration)
  ├─ AppPlugin API (hook contract)
  └─ Legacy adapter (temporary)

truvis-renderer
  └─ Render backend services

truvis-app/render_pipeline (temporary)
  └─ existing passes (to be logically decoupled first)
```

## Migration Plan

1. 建立新命名入口（type alias / module alias / re-export）并保持旧调用可编译。
2. 将 `big_update` 拆为显式 phase 函数，保证行为等价。
3. 迁移 scene/asset 更新触发逻辑出 `Renderer`，由 runtime phase 调度。
4. 引入可注册 overlay，替换 runtime 内硬编码 UI。
5. 逐个迁移 demo 到 `AppPlugin` 新契约。
6. 标记旧 `OuterApp` 接口 deprecated，验证兼容窗口后移除。

## Risks / Trade-offs

- 过渡期会出现“双命名并存”（`RenderApp`/`FrameRuntime`）和“双接口并存”（`OuterApp`/`AppPlugin`），短期认知负担上升。
- phase 拆分若不保持顺序一致，可能造成输入、GUI、resize 时序回归。
- `Renderer` 职责收敛过程中，需要防止 runtime 反向变成新的“God object”。

缓解策略：小步迁移、每阶段可运行、四个 demo 全量回归。
