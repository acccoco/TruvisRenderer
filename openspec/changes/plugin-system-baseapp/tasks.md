## 0. OpenSpec 基线同步

- [x] 0.1 同步 `render-backend-lifecycle-ctx`：移除 `submit_gui_data` / `register_gui_font` 作为 RenderBackend API 的要求
- [x] 0.2 同步 `layered-frame-orchestration`：三层模型从 App / FrameRuntime / RenderBackend 改为 App / BaseApp / RenderBackend
- [x] 0.3 同步 `gui-pass-separation`：GUI RenderGraph adapter 迁入 GuiPlugin 上层集成 crate，`truvis-gui-backend` 保持不依赖 RenderGraph
- [x] 0.4 同步 `render-threading`：render loop 通过 `Box<dyn FrameApp>` 驱动，不再创建 `FrameRuntime`

## 1. Plugin Trait 和 Ctx 类型

- [x] 1.1 在 `truvis-frame-api` 中定义 `Plugin` trait（init / on_input / update / on_resize / shutdown，均有默认空实现）
- [x] 1.2 在 `truvis-frame-api` 中定义 `PluginInitCtx`、`PluginUpdateCtx`、`PluginRenderCtx`、`PluginResizeCtx` 类型
- [x] 1.3 移除 `RenderCtx` 中的 `gui_draw_data` 字段；Plugin render Ctx 不包含 imgui 类型
- [x] 1.4 明确 Plugin 特有能力（如 `contribute_passes` / `ui`）通过具体类型方法暴露，不加入统一 `Plugin` trait

## 2. FrameApp 和 FrameAppHooks Trait

- [x] 2.1 在 `truvis-frame-api` 中定义 `FrameApp` trait（init_after_window / run_frame / push_input_event / recreate_swapchain_if_needed / time_to_render / shutdown）
- [x] 2.2 确保 `FrameApp::shutdown(&mut self)` 可通过 `Box<dyn FrameApp>` 调用
- [x] 2.3 在 `truvis-frame-api` 中定义 `FrameAppHooks` trait（on_input / update / render / camera），其中 `render(&mut self, ...)` 允许 Plugin 准备 per-frame GPU 数据
- [x] 2.4 移除 `FramePlugin` trait 定义和旧 `InitCtx` / `UpdateCtx` / `RenderCtx` / `ResizeCtx` 命名入口

## 3. BaseApp 帧骨架

- [x] 3.1 在 `truvis-frame-runtime` 中创建 `BaseApp` struct（持有 RenderBackend + 输入事件队列）
- [x] 3.2 实现 `BaseApp::run_frame(&mut self, app: &mut impl FrameAppHooks)` 帧骨架方法
- [x] 3.3 实现 `BaseApp` 的 `init_after_window`（调 RenderBackend init，返回 Ctx）、`time_to_render`、`push_input_event`、resize、destroy 等生命周期方法
- [x] 3.4 确保 BaseApp 不持有 Plugin、GUI、Camera、Overlay、InputState 或 demo 特定状态
- [x] 3.5 移除 `FrameRuntime` struct 及其实现
- [x] 3.6 迁移 `FrameRuntime::init_env` 的职责到合适入口（如 `truvis-frame-runtime` free function 或 `WinitApp` 启动路径）

## 4. GuiPlugin

- [x] 4.1 创建 `truvis-gui-plugin` 或等价上层集成 crate，承载 `GuiPlugin` 和 GUI RenderGraph adapter
- [x] 4.2 创建 `GuiPlugin` struct，将 `GuiHost` 的 imgui context 管理迁入
- [x] 4.3 将 `GuiBackend` 的 GPU mesh buffer / tex_map 管理迁入 `GuiPlugin`，从 `RenderPresent` 中移除 `gui_backend` 字段
- [x] 4.4 移除 `RenderBackend::submit_gui_data` 和 `RenderBackend::register_gui_font` 方法
- [x] 4.5 实现 `Plugin` trait（init 中注册 font texture，on_input 转发事件并返回消费状态）
- [x] 4.6 实现 `GuiPlugin` 特有方法：`begin_frame` / `ui()` / `end_frame` / `prepare_render_data`
- [x] 4.7 实现 `GuiPlugin::contribute_passes`，封装 `GuiRgPass` 的 render graph 注入逻辑
- [x] 4.8 移除 `GuiHost`（`gui_front.rs`）和 demo 侧手写 `GuiRgPass` 使用点

## 5. Overlay 迁移

- [x] 5.1 将 `DebugInfoOverlay` 改为实现 `Plugin` trait 的独立 struct，暴露 `build_overlay_ui(ui, camera, extent, accum_frames)` 特有方法
- [x] 5.2 将 `PipelineControlsOverlay` 改为实现 `Plugin` trait 的独立 struct，暴露 `build_overlay_ui(ui, pipeline_settings)` 特有方法
- [x] 5.3 移除 `overlay.rs` 中的 `OverlayModule` trait 和 `default_overlays()`

## 6. 渲染管线 Plugin 迁移

- [x] 6.1 将 Triangle 渲染能力拆为 App 持有的具体 Plugin，负责自身 pipeline/pass 资源与 RenderGraph pass 贡献
- [x] 6.2 将 ShaderToy 渲染能力拆为 App 持有的具体 Plugin，负责自身 pipeline/pass 资源与 RenderGraph pass 贡献
- [x] 6.3 将 RT Cornell/Sponza 共用的 `RtPipeline` 改为 App 持有的具体 Plugin，负责自身 pipeline/pass 资源与 RenderGraph pass 贡献
- [x] 6.4 确保 App 在 `FrameAppHooks::render` 中统一创建/执行 RenderGraph，并显式决定 GUI 与渲染管线 pass 顺序

## 7. render_loop 适配

- [x] 7.1 修改 `truvis-winit-app` 的 render_loop，使用 `Box<dyn FrameApp>` 替代 `FrameRuntime`
- [x] 7.2 新增 `WinitApp::run_app(|| Box<dyn FrameApp>)` 入口，替代语义不准确的 `run_plugin`
- [x] 7.3 将输入事件转发到 `FrameApp::push_input_event`
- [x] 7.4 在退出路径调用 `FrameApp::shutdown(&mut self)`，保持 Window 在 Vulkan 资源销毁后再 drop

## 8. Demo App 迁移

- [x] 8.1 迁移 `HelloTriangleApp`：实现 `FrameApp` + `FrameAppHooks`，持有 BaseApp + GuiPlugin + TrianglePlugin + Camera/Input state
- [x] 8.2 迁移 `ShaderToy`：实现 `FrameApp` + `FrameAppHooks`，持有 BaseApp + GuiPlugin + ShaderToyPlugin + Camera/Input state
- [x] 8.3 迁移 `CornellApp`：实现 `FrameApp` + `FrameAppHooks`，持有 BaseApp + GuiPlugin + RtPipelinePlugin + Camera/Input state
- [x] 8.4 迁移 `SponzaApp`：实现 `FrameApp` + `FrameAppHooks`，持有 BaseApp + GuiPlugin + RtPipelinePlugin + Camera/Input state
- [x] 8.5 移除 `truvis-app` 中的 `gui_rg_pass.rs`（逻辑已迁入 GuiPlugin 上层集成 crate）

## 9. 文档和清理

- [x] 9.1 更新 `ARCHITECTURE.md`，描述 App / BaseApp / Plugin 三层架构
- [x] 9.2 更新 `README.md` 和模块 README，替换 `FrameRuntime` / `FramePlugin` / `run_plugin` 叙事
- [x] 9.3 更新 `truvis-frame-api`、`truvis-frame-runtime`、GUI plugin crate 的模块文档
- [x] 9.4 清理 `truvis-frame-runtime` 中残留的 `gui_front.rs`、`camera_controller.rs`、`input_manager.rs`（如已迁入 App/Plugin）
- [x] 9.5 验证 `cargo check --all` 通过，无未使用依赖
