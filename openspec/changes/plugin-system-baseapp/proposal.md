## Why

当前 `FrameRuntime` 硬编码了 GUI（`GuiHost`）、Camera 和单一 `FramePlugin`，导致三个问题：(1) GUI 无法按需加载或替换——即使 app 不需要 GUI，`RenderCtx` 仍携带 `gui_draw_data`；(2) app 只能有一个 FramePlugin，内部组合只能靠自定义 struct 而非框架层面的复用；(3) 每个 app 必须在 render graph 中手动接入 `GuiRgPass`，与 runtime 层的 GUI 编排形成双重管理。引入 Plugin 系统和 BaseApp 骨架，让 app 自由组合可复用的能力单元，同时消除框架层面的硬编码。

## What Changes

- **引入统一 `Plugin` trait**：定义 Plugin 的标准生命周期（init / on_input / update / on_resize / shutdown），所有可复用能力单元遵循此契约。Plugin 可额外暴露特有方法供 App 调用。
- **`BaseApp` 替代 `FrameRuntime` 的帧骨架职责**：`BaseApp` 只持有 `Renderer` 和待处理输入事件队列，提供不变的帧执行序列（begin_frame → hooks → present → end_frame），通过 `FrameAppHooks` trait 回调 App。**BREAKING**：`FrameRuntime` 被移除。
- **`FrameApp` trait 替代 `FramePlugin`**：面向 render_loop 的外部契约。App 实现 `FrameApp`（init / run_frame / on_resize / shutdown）和 `FrameAppHooks`（on_input / update / render / camera），在 hook 内自主编排持有的 Plugins。**BREAKING**：`FramePlugin` trait 被移除。
- **GUI 从 runtime infra 降级为普通 Plugin**：`GuiPlugin` 封装 imgui context + input + font + GPU mesh upload + render pass。App 在 update hook 中调用 `gui.begin_frame()` / `gui.ui()` / `gui.end_frame()`，在 render hook 中调用 `gui.contribute_passes()`。runtime 不再硬编码 GUI。
- **特定渲染管线 Plugin 化**：Triangle、ShaderToy、RT Cornell/Sponza 等渲染能力由具体 Plugin 持有 pipeline/pass/GPU 资源，并通过特有 `contribute_passes` 等方法向 App 的 RenderGraph 编排贡献 pass。
- **RenderGraph 由 App 在 render hook 中构建**：App 在 `FrameAppHooks::render` 中创建 `RenderGraphBuilder`，调用各 Plugin 的 `contribute_passes` 方法，自主决定 pass 顺序和拓扑结构。
- **Camera 从 runtime 移入 App**：Camera 和 CameraController 成为 App 的自有字段，不再由 runtime 持有。App 通过 `FrameAppHooks::camera()` 暴露给 BaseApp 的 prepare 阶段。
- **`OverlayModule` 合并入 Plugin 体系**：现有 overlay 改为实现 `Plugin` trait + `UiContributor`，由 App 自行持有和调用。
- **四个 demo app 迁移到新架构**。

## Capabilities

### New Capabilities
- `plugin-trait`: 统一 Plugin trait 的定义、生命周期契约和 Ctx 类型
- `base-app-frame-skeleton`: BaseApp 结构、帧骨架执行序列、FrameApp / FrameAppHooks trait 定义
- `gui-plugin`: 将 GUI 从 runtime infra 提取为独立 Plugin，包含 imgui context 管理和 render graph pass 贡献
- `render-pipeline-plugin`: 将 demo 特定渲染管线收敛为 App 持有的具体 Plugin

### Modified Capabilities
- `frame-runtime-boundary`: FrameRuntime 被 BaseApp 替代，帧编排入口和 phase 语义变化
- `runtime-api-boundary`: FramePlugin 被 FrameApp + Plugin 替代，typed contexts 适配新架构
- `layered-frame-orchestration`: 三层模型从 App / FrameRuntime / Renderer 调整为 App / BaseApp / Renderer，GUI draw data 不再由 runtime 拼接
- `renderer-lifecycle-ctx`: Renderer 不再提供 GUI 数据注入点，Renderer Ctx 仍作为 App 裁剪 Plugin Ctx 的来源
- `gui-pass-separation`: GUI render-graph 适配从 demo app 迁入 GUI Plugin 所在的上层集成 crate，低层 `truvis-gui-backend` 仍不依赖 render graph
- `render-threading`: render loop 通过 `Box<dyn FrameApp>` 驱动 App，而非创建 `FrameRuntime`

## Impact

- **crate 变动**：`truvis-frame-runtime`（BaseApp 替代 FrameRuntime）、`truvis-frame-api`（Plugin trait + FrameApp trait 替代 FramePlugin）、`truvis-gui-plugin` 或等价上层集成 crate（GuiPlugin 封装）、`truvis-app`（demo 迁移为 App + pipeline plugins）
- **BREAKING API**：`FramePlugin` trait 移除，`FrameRuntime::new_with_plugin` 移除，`RenderCtx::gui_draw_data` 移除，`Renderer::submit_gui_data` / `register_gui_font` 移除，`RenderPresent::gui_backend` 字段移除
- **render_loop 调用方式变化**：从 `FrameRuntime::new_with_plugin` + 调用 `run_frame` 变为创建 `Box<dyn FrameApp>`，只通过 `FrameApp` API 驱动
- **文档**：`ARCHITECTURE.md` 需更新三层模型为 BaseApp / App / Plugin 架构
