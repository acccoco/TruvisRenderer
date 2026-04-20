## Context

当前 `FrameRuntime` 改造已经完成第一阶段目标（命名、phase、兼容迁移），但体系结构仍有三个关键缺口：

1. 插件能力边界未类型化（`AppPlugin` 仍持有 `Renderer` 全量可见性）
2. runtime 编排边界未封装（外部路径可绕过 runtime 直接访问内部状态）
3. crate 物理边界未落地（`truvis-app` 仍是框架 + 示例 + pass 的聚合体）

本 change 聚焦把“约定边界”升级为“类型边界 + crate 边界”。

## Goals / Non-Goals

**Goals**

- 建立 `AppPlugin` 的 typed contexts 契约，替代直接暴露 `Renderer`。
- 让 `FrameRuntime` 成为唯一可观测的帧编排入口。
- 完成 demo 迁移并验证行为等价。
- 按低风险顺序完成 `truvis-app-api`、`truvis-frame-runtime`、`truvis-render-passes` 拆分。
- 在收口条件满足后移除兼容层。

**Non-Goals**

- 不修改 `render-threading` 的线程模型、事件通道模型、退出握手模型。
- 不在本 change 全量去除 `Gfx::get()` 单例。
- 不引入 Main World / Render World 双 world 体系。

## Decisions

### 决策 1：先类型收口，再物理拆分

先在现有 crate 内完成接口收口（typed contexts + runtime 单入口），确认行为稳定后再做 crate 迁移。

原因：避免“语义变化 + 物理搬迁”同时发生造成高回归风险。

### 决策 2：`AppPlugin` 采用阶段化上下文

定义 `InitCtx` / `UiCtx` / `UpdateCtx` / `RenderCtx` / `ResizeCtx`（命名可在实现时微调），每个 hook 只暴露该阶段需要的稳定能力。

原因：把边界约束从“文档约定”变为“类型系统约束”。

### 决策 3：`FrameRuntime` 对外提供单入口 API

render loop 不再直接访问 runtime 内部字段；通过 runtime API 完成输入灌入、resize 判定、单帧推进。

原因：保证 phase 编排路径唯一，便于验证不变量。

### 决策 3.1：prepare 阶段唯一归属 runtime

prepare 阶段继续由 `FrameRuntime` 驱动 backend/runtime 侧准备工作，不在 `AppPlugin` 契约中新增独立 prepare hook。

原因：避免与 `update/render` 形成职责交叠，防止双入口与时序歧义。

### 决策 4：compatibility window 只保留到 M5

兼容层在 M1~M4 作为迁移缓冲存在，M5 作为明确的下线里程碑执行移除。

原因：避免 deprecated 接口长期滞留导致边界回流。

### 决策 5：文档/注释/命名重命名与代码改造同批验收

每个里程碑的“定义完成（DoD）”包含三项同步动作：

1. 更新受影响模块文档（README、ARCHITECTURE、OpenSpec）
2. 更新关键边界注释（phase 顺序、hook 语义、兼容窗口状态）
3. 对语义已稳定但命名滞后的文件/模块执行重命名，并提供兼容导出或迁移说明

原因：如果这些动作延后，结构语义会再次漂移，增加后续维护和迁移成本。

### 决策 6：与既有 OpenSpec 能力保持一致，避免交叉冲突

- `gui-pass-separation` 继续生效：`GuiRgPass` 保持应用集成层，不下沉到 gui-backend。
- `render-context-split` 继续生效：typed contexts 不得退化为对 `Renderer` 内部布局的旁路暴露。

原因：避免新 change 在收敛边界时反向破坏已达成的解耦成果。

### 决策 7：迁移采用 move + shim，禁止并行复制实现

模块迁移到新 crate 时，旧路径仅保留转发层（re-export/shim）或在同里程碑移除，不保留长期并行实现。

原因：并行实现会显著增加回归面与维护成本，并使边界决策失效。

## Target Architecture (End State)

```text
truvis-winit-app (platform host)
  -> truvis-frame-runtime (phase orchestration only)
      -> truvis-app-api (AppPlugin + typed contexts + overlay contracts)
      -> truvis-renderer (backend only)
  -> truvis-render-passes (shared passes)
  -> demo apps (triangle / rt-cornell / rt-sponza / shader-toy)
```

## Milestones

### M1：AppPlugin 上下文类型化（兼容保留）

- 建立 typed contexts 与新 trait 签名
- 保留兼容桥接，确保旧路径可编译
- 不改变运行时行为

### M2：FrameRuntime 封装化 + 单入口

- 收敛 runtime 字段可见性
- render loop 改为仅通过 runtime API 驱动
- 统一帧节流决策点，移除重复判定

### M3：Demo 迁移

- 四个 demo 全量迁移到新上下文契约
- 移除对 `Renderer` 内部字段布局的直接依赖

### M4：crate 物理拆分

- 拆分 `truvis-app-api` 与 `truvis-frame-runtime`
- 调整 `truvis-winit-app` 依赖与导入路径
- 保持过渡期 re-export 兼容
- 同步完成涉及目录与文件的语义重命名（如 runtime/plugin 入口文件名）

### M5：pass 迁移 + 兼容层移除

- 迁移 `render_pipeline/*` 到 `truvis-render-passes`
- 下线 `OuterApp` / `LegacyOuterAppAdapter` / `RenderApp` / `WinitApp::run`
- 完成文档与 spec 对齐
- 完成注释清理与重命名收口（移除过期迁移注释、保留最终边界注释）
- `GuiRgPass` 继续保持在应用集成层，保证与 `gui-pass-separation` 一致

## Invariants

- phase 顺序保持：`input -> build_ui -> update -> prepare -> render -> present`
- resize/out-of-date 重建仍通过 runtime 单一入口
- 线程模型与关闭握手持续满足 `render-threading` 规范
- 每个 phase 在单帧中至多执行一次

## Compatibility Window Exit Criteria

1. 四个 demo 均迁移到 typed contexts 契约。
2. `truvis-winit-app` 默认路径不再依赖旧接口。
3. `cargo check --all` 与 demo 回归通过。
4. 兼容层接口移除并完成文档、注释、文件命名更新。

## Risks / Trade-offs

- typed contexts 首次引入会提高短期改造成本（插件改签名）。
- crate 拆分可能暴露历史隐式依赖，需要分批提取。
- 兼容层移除时若外部仍有依赖，需要提前公告迁移窗口。

缓解：按里程碑小步推进、每个里程碑独立可回滚、每步完成即验证。
