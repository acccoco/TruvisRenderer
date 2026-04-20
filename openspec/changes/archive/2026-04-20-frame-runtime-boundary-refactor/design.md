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

### 决策 1：命名收敛（已完成）

- `RenderApp` -> `FrameRuntime`（已落地，`RenderApp` 标记 deprecated）
- `OuterApp` -> `AppPlugin`（已落地，`OuterApp` 标记 deprecated，四个 demo 已迁移）
- `Renderer` 暂不强制改名，但 API 语义按 backend 收敛（doc 已更新为 "渲染 Backend 核心"）

原因：先统一语义，再做文件与 crate 迁移，可降低改动面和回滚复杂度。

### 决策 2：`AppPlugin` 采用“单 trait 多 hook”

在过渡阶段保留单 trait，按帧阶段暴露 hook（如 update/build_ui/extract/render），不立即拆成 `SceneProvider` + `RenderSubsystem` 两个 trait。

原因：兼容成本最低，先把行为边界做清，再决定是否做双 trait 的长期抽象。

### 决策 3：`FrameRuntime` 持有编排职责，不持有硬编码业务（已完成）

`FrameRuntime` 负责阶段顺序与上下文装配。默认 overlay UI 已从硬编码抽离为可注册 `OverlayModule`（`DebugInfoOverlay` / `PipelineControlsOverlay`），通过 `add_overlay` / `clear_overlays` 管理。

### 决策 4：`Renderer` 只保留 backend 职责（已完成）

`Renderer` 负责 device/swapchain/cmd/sync/submit/present 等 GPU 路径，不再主动推进 scene/asset 的 world 更新逻辑。

- `asset_hub.update()` 已从 `begin_frame()` 迁出，由 `FrameRuntime::begin_frame` 显式调度 `update_assets()`。
- `accum_data.update_accum_frames()` 已从 `before_render()` 迁出，由 `FrameRuntime::phase_prepare` 显式调度 `update_accum_frames()`。

### 决策 5：兼容迁移优先于一次性切换（已完成）

引入 `LegacyOuterAppAdapter`，确保 `triangle`、`rt-cornell`、`rt-sponza`、`shader-toy` 逐步迁移。

四个 demo 已全部迁移到原生 `AppPlugin`。`OuterApp` / `LegacyOuterAppAdapter` / `WinitApp::run` / `FrameRuntime::new` 均标记 deprecated，待下一 change 移除。

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

## AppPlugin Hook Contract (Transition)

过渡态的 `AppPlugin` 需要覆盖当前 `OuterApp` 的关键能力，至少包含以下 hook 语义：

- `init`：初始化渲染相关资源与场景初始状态（对应旧 `OuterApp::init`）。
- `build_ui`：每帧 UI 构建入口（对应旧 `OuterApp::draw_ui`）。
- `update`：每帧 CPU 侧更新入口（输入、相机、业务逻辑，对应旧 `OuterApp::update`）。
- `render`：每帧图构建/命令提交入口（对应旧 `OuterApp::draw`）。
- `on_resize`：swapchain 重建后触发的资源重建入口（对应旧 `OuterApp::on_window_resized`）。
- `shutdown`（建议）：资源释放与退出前收尾，用于替代历史上隐式 drop 假设。

调用顺序约束（单帧）：

`input -> build_ui -> update -> prepare -> render -> present`

其中 `on_resize` 不在每帧固定触发，仅在 resize/out-of-date 重建成功后触发。

## Runtime / Renderer Responsibility Matrix

为避免重构后职责再次回流，过渡态按以下边界执行：

- `FrameRuntime` 负责：阶段编排、插件调用顺序、scene/asset 更新调度决策、overlay 模块注册与调用。
- `Renderer` 负责：device/swapchain/cmd/sync/submit/present、GPU 数据上传执行、descriptor 更新执行。
- `AppPlugin` 负责：业务场景构建、渲染策略、UI 逻辑、按 runtime 约定读写上下文。

注：过渡态下，`AppPlugin` 仍通过 `Renderer` 的稳定接口子集访问上下文能力；
“强类型受控上下文”作为后续 change 的收敛方向。

约束：

- `AppPlugin` 不应把 `Renderer` 的内部字段布局当作稳定接口。
- `Renderer` 不应主动触发应用 world 生命周期决策。
- `FrameRuntime` 作为编排器不应重新变成承载业务细节的 God object。

## Phase Invariants and Observability

为了验证“拆分 phase 但行为等价”，需要固化以下不变量：

- 每帧每个 phase 至多执行一次。
- resize/out-of-date 路径通过单一重建入口触发，避免并行分叉流程。
- GUI、resize、present 的时序与现有行为等价。
- 线程模型与关闭握手继续满足 `render-threading` 既有规范。

建议在阶段入口保留 tracy span 与关键日志，作为回归比对依据。

## Compatibility Window Exit Criteria

兼容窗口结束需满足全部条件：

1. 四个 demo（`triangle` / `rt-cornell` / `rt-sponza` / `shader-toy`）都改为 `AppPlugin` 新路径。
2. `truvis-winit-app` 默认接入路径不再依赖旧 `OuterApp` 类型。
3. 回归验证通过（启动、交互、resize、关闭握手）。
4. 旧接口标记状态从“兼容中”更新为“待移除”，并明确下一 change 的移除任务。

## Risks / Trade-offs

- 过渡期会出现“双命名并存”（`RenderApp`/`FrameRuntime`）和“双接口并存”（`OuterApp`/`AppPlugin`），短期认知负担上升。
- phase 拆分若不保持顺序一致，可能造成输入、GUI、resize 时序回归。
- `Renderer` 职责收敛过程中，需要防止 runtime 反向变成新的“God object”。

缓解策略：小步迁移、每阶段可运行、四个 demo 全量回归。
