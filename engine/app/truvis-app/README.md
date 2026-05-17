# truvis-app

应用集成层，承载示例应用、GUI plugin、overlay plugin 与具体 render pipeline plugin。

核心契约与帧骨架位于独立 crate：

- App / Plugin 契约与 typed contexts：`truvis-frame-api`
- 帧骨架与 App shell：`truvis-frame-runtime::RenderAppShell`
- 通用 render pass：`truvis-render-passes`

## 主要内容

- 示例应用实现（triangle / rt-cornell / rt-sponza / shader-toy）
- `GuiPlugin`：imgui context、输入转发、字体资源、GUI mesh 上传和 GUI RenderGraph pass 注入
- `DebugInfoOverlay` / `PipelineControlsOverlay`：实现 `Plugin` 的 UI-only 插件
- `TrianglePlugin` / `ShaderToyPlugin` / `RtPipeline`：由 App 持有的具体渲染能力插件

## 使用方式

- demo state 实现 `RenderAppHooks`，持有 GUI、相机/输入状态、overlay 和具体 render pipeline plugin
- demo state 通过 `visit_plugins_mut` 声明标准生命周期 Plugin 顺序，由 `RenderAppShell` 批量调用 init / update / resize / shutdown
- `src/bin/` 入口用 `RenderAppShell::new(demo_state)` 包装成 `Box<dyn RenderApp>` 后交给 `truvis-winit-app::WinitApp::run_app(...)`

## 帧内编排

- `RenderAppHooks::on_input` 负责 App 级输入策略，例如先让 `GuiPlugin` 判断是否消费事件，再把未消费事件交给 camera/input state。
- `RenderAppHooks::update` 负责 CPU 状态更新和 UI frame 构建；标准 Plugin update 由 `RenderAppShell` 通过 visitor 统一调用。
- `RenderAppHooks::render` 创建 per-frame `RenderGraphBuilder`，再按 App 语义显式调用具体 Plugin 的 pass 贡献方法。
- GUI overlay 的位置由 App 决定：Triangle / ShaderToy 在主渲染 pass 后添加 GUI pass，RT demo 先执行 compute graph，再在 present graph 中 resolve 并叠加 GUI。

## GUI 与渲染管线 Plugin

- `GuiPlugin` 位于本集成层，管理 imgui context、输入消费、font texture、GUI mesh buffer、draw data 和 GUI RenderGraph adapter。
- `DebugInfoOverlay` / `PipelineControlsOverlay` 是 UI-only Plugin，只参与标准生命周期，不直接拥有 RenderGraph pass。
- `TrianglePlugin` / `ShaderToyPlugin` / `RtPipeline` 通过 `Plugin` 生命周期管理自身 GPU 资源，并通过具体方法暴露 `contribute_passes` 等特有能力。
- App 通过组合具体类型组织渲染能力，不使用 downcast、注册表或消息总线。

## 边界约束

- 本层承载 demo 与集成逻辑，不向底层反向注入依赖
- `RenderAppShell` 不持有 GUI、Camera、Overlay 或具体渲染管线
- App state 在 `RenderAppHooks::render` 中创建 RenderGraph，并显式决定渲染管线与 GUI pass 顺序
- GUI UI 构建、输入消费和 RenderGraph pass 贡献仍由 App 通过具体 Plugin 类型显式编排
