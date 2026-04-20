## Why

`frame-runtime-boundary-refactor` 已完成命名收敛与阶段化改造，但当前工程仍处于“过渡态”：

- `AppPlugin` 仍直接依赖 `Renderer` 大对象，能力边界主要靠注释约定
- `FrameRuntime` 对外暴露字段较多，render loop 可越级访问内部状态
- `truvis-app` 同时承载 runtime、demo、pass，物理 crate 边界尚未落地
- 兼容层（`OuterApp` / `LegacyOuterAppAdapter` / `RenderApp`）仍在主干路径中可见

这些问题会放大后续重构成本，并降低“接口稳定性 vs 内部演进”之间的解耦能力。

## What Changes

- 把 `AppPlugin` 从“直接拿 `Renderer`”迁移为“按阶段拿受控上下文（typed contexts）”。
- 将 `FrameRuntime` 收敛为唯一帧编排入口，render loop 仅通过 runtime API 驱动。
- 完成四个 demo 对新上下文契约的迁移，清理对 `Renderer` 内部字段布局的直接依赖。
- 同步维护代码注释与模块文档，确保注释语义与阶段边界、调用顺序一致。
- 新增并迁移 crate 边界：
  - `truvis-app-api`：插件契约、上下文类型、overlay 合约
  - `truvis-frame-runtime`：帧编排运行时
  - `truvis-render-passes`：通用 pass 实现
- 对命名语义已稳定但文件名/模块名仍滞后的路径执行重命名，并保持过渡期导入兼容。
- 保持与既有 OpenSpec 能力一致（`gui-pass-separation`、`render-context-split`），避免分层回退。
- 迁移过程采用 move + shim，避免并行维护两套等价实现。
- 在验收通过后下线兼容层接口，完成过渡窗口收口。

## Capabilities

### New Capabilities

- `runtime-api-boundary`：定义 AppPlugin 上下文边界、FrameRuntime 单入口边界、crate 拆分边界及兼容层退出条件。

## Impact

- `engine/crates/truvis-app/src/app_plugin.rs` 及 plugin 实现：改造为上下文化接口。
- `engine/crates/truvis-app/src/render_app.rs` 与 `truvis-winit-app/src/render_loop.rs`：收敛为 runtime 单入口交互。
- `engine/crates/truvis-app/src/render_pipeline/*`：迁移目标为 `truvis-render-passes`。
- 文件与模块命名：按里程碑推进重命名（如 runtime 语义入口、plugin 目录语义入口），并更新导出路径。
- 新增 crate：`truvis-app-api`、`truvis-frame-runtime`、`truvis-render-passes`（按里程碑逐步接入）。
- 文档：`README.md`、`ARCHITECTURE.md`、模块 README 与 OpenSpec 文档同步更新。
- 注释：phase 注释、hook 注释、迁移注释（deprecated/compatibility）与实现保持一致。
- 线程约束：`render-threading` 现有规范继续生效，本 change 不改变线程协议与关闭握手。
