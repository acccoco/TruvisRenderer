# truvis-app-frame

`truvis-app-frame` 定义 App 框架层的契约、帧骨架和渲染线程主循环。它把 App
契约、runtime shell 与平台无关的 render loop 合并到同一 crate 中，供平台入口
和具体 app state 共同使用。

## 主要职责

- `RenderApp`：render loop 看到的 object-safe App 契约，通常由 `RenderAppShell` 实现。
- `RenderAppHooks`：`RenderAppShell` 在 init / input / update / after_prepare / render / resize / shutdown 阶段回调具体 App 的 hook 契约。
- `Plugin`：可复用能力单元的标准生命周期契约，覆盖 init / input / update / resize / shutdown。
- `PluginInitCtx` / `PluginUpdateCtx` / `PluginRenderCtx` / `PluginResizeCtx` / `PluginShutdownCtx`：从 `RenderRuntime*Ctx` 裁剪出的 plugin-facing context。
- `InputEvent`：平台输入事件的引擎侧表示
- `RenderAppShell`：持有 `RenderRuntime` 与待处理输入队列，执行固定帧顺序。
- `render_loop` / `SharedState` / `RenderInitMsg`：渲染线程入口与平台入口共享的线程状态。

## 设计意图

- render loop 只依赖 `RenderApp`，因此平台层可以持有 `Box<dyn RenderApp>`，而不需要知道具体 App、Plugin 或 `RenderRuntime`。
- 具体 App 通过 `RenderAppHooks` 暴露固定 hook 点，同时继续自行持有 GUI、camera/input state、overlay 和 render pipeline plugin。
- `after_prepare` 是 App 可选同步查询点，发生在 runtime prepare 完成后、render graph 组图前，用于调用同步 raycast 等依赖 GPU scene 快照的接口。
- `RenderAppHooks::visit_plugins_mut` / `visit_plugins_mut_rev` 只暴露标准生命周期 Plugin，方便 runtime shell 批量调用 init / update / resize / shutdown。
- Plugin 的特有能力不放进统一 trait，例如 `GuiPlugin::ui`、`GuiPlugin::contribute_passes`、`RtPipeline::contribute_compute_passes` 仍由 App 通过具体类型显式调用。
- winit 等平台后端只负责窗口、事件循环和事件适配；本 crate 不依赖 `winit`。

## Ctx 边界

- `PluginInitCtx` 同时携带 `World`、`GpuStore` 和初始化所需的 typed `Gfx` Ctx，用于创建 App/Plugin 持有的 GPU 资源。
- `PluginUpdateCtx` 面向 CPU 更新，提供 `World` 和帧设置相关状态，不承担 command recording。
- `RenderRuntimeRayCastCtx` 只在 App `after_prepare` hook 中出现，不进入通用 Plugin Ctx。
- `PluginRenderCtx` 面向渲染录制，提供只读 `GpuStore` 与 command/queue 相关能力，不包含 GUI draw data。
- `PluginResizeCtx` 和 `PluginShutdownCtx` 用于重建或释放 Plugin-owned GPU 资源，manager-owned image/view 必须通过 `GpuStore` 中的 manager 释放。

## 边界约束

- 不创建平台窗口，不处理 winit lifecycle，不持有具体 app/plugin 业务状态
- `Plugin` trait 不包含 `ui`、`build_ui`、`contribute_passes` 等特有能力
- `RenderAppHooks::visit_plugins_mut` 只暴露标准生命周期 Plugin；特有能力仍通过具体 Plugin 类型显式调用
- GUI draw data 不进入通用 Plugin Ctx，由具体 `GuiPlugin` 自行管理
