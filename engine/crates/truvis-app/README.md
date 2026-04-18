# truvis-app

应用框架层，负责 `FrameRuntime` 帧编排与 `AppPlugin` 扩展契约，并承载示例应用实现。

## 主要内容

- `FrameRuntime`：显式 phase 调度（`input -> build_ui -> update -> prepare -> render -> present`）
- `AppPlugin`：应用生命周期 hook（`init/build_ui/update/render/on_resize/shutdown`）
- overlay 注册系统：默认 `DebugInfoOverlay` + `PipelineControlsOverlay`，可通过 `add_overlay/clear_overlays` 自定义
- 示例应用实现（triangle / rt-cornell / rt-sponza / shader-toy）

## 使用方式

- 新代码：实现 `AppPlugin` 并通过 `truvis-winit-app::WinitApp::run_plugin(...)` 启动
- 兼容路径：`OuterApp` / `LegacyOuterAppAdapter` / `FrameRuntime::new` 为 deprecated，后续 change 移除

## 边界约束

- 本层负责 app 语义编排，不向底层注入平台细节
- `Renderer` 保持 backend 语义；scene/asset 调度由 `FrameRuntime` phase 决策
