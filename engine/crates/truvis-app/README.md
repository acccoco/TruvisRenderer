# truvis-app

应用集成层，承载示例应用、GUI plugin、overlay plugin 与具体 render pipeline plugin。

核心契约与帧骨架位于独立 crate：
- App / Plugin 契约与 typed contexts：`truvis-frame-api`
- 帧骨架与 App shell：`truvis-frame-runtime::BaseApp` / `FrameAppShell`
- 通用 render pass：`truvis-render-passes`

## 主要内容

- 示例应用实现（triangle / rt-cornell / rt-sponza / shader-toy）
- `GuiPlugin`：imgui context、输入转发、字体资源、GUI mesh 上传和 GUI RenderGraph pass 注入
- `DebugInfoOverlay` / `PipelineControlsOverlay`：实现 `Plugin` 的 UI-only 插件
- `TrianglePlugin` / `ShaderToyPlugin` / `RtPipeline`：由 App 持有的具体渲染能力插件

## 使用方式

- demo state 实现 `FrameAppState` + `FrameAppHooks`，持有 GUI、相机/输入状态、overlay 和具体 render pipeline plugin
- `src/bin/` 入口用 `FrameAppShell::new(demo_state)` 包装成 `Box<dyn FrameApp>` 后交给 `truvis-winit-app::WinitApp::run_app(...)`

## 边界约束

- 本层承载 demo 与集成逻辑，不向底层反向注入依赖
- `BaseApp` 不持有 GUI、Camera、Overlay 或具体渲染管线
- App state 在 `FrameAppHooks::render` 中创建 RenderGraph，并显式决定渲染管线与 GUI pass 顺序
