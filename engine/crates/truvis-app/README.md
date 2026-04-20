# truvis-app

应用集成层，承载示例应用、RenderGraph 适配（`GuiRgPass`、`RtPipeline`）以及过渡期 re-export shim。

核心契约与运行时已迁出至独立 crate：
- 插件契约与 typed contexts：`truvis-app-api`
- 帧编排运行时：`truvis-frame-runtime`
- 通用 render pass：`truvis-render-passes`

## 主要内容

- 示例应用实现（triangle / rt-cornell / rt-sponza / shader-toy）
- `GuiRgPass`：ImGui RenderGraph 适配（应用集成层，不下沉到 gui-backend）
- `RtPipeline`（`rt_render_graph`）：RT 流水线编排
- Re-export shim：`app_plugin`、`frame_runtime`、`overlay`、`render_pipeline/*` 转发到新 crate

## 使用方式

- 实现 `AppPlugin`（from `truvis-app-api`）并通过 `truvis-winit-app::WinitApp::run_plugin(...)` 启动

## 边界约束

- 本层承载 demo 与集成逻辑，不向底层反向注入依赖
- `Renderer` 保持 backend 语义；scene/asset 调度由 `FrameRuntime` phase 决策
