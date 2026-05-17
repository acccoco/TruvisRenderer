# truvis-frame-api

Frame API crate 定义 render loop、runtime shell、App hooks 和 Plugin 之间的稳定契约。本 crate 只描述接口与 typed contexts，不持有运行时状态，也不实现帧循环。

## 主要职责

- `RenderApp`：render loop 看到的 object-safe App 契约，通常由 `truvis-frame-runtime::RenderAppShell` 实现。
- `RenderAppHooks`：`RenderAppShell` 在 init / input / update / render / resize / shutdown 阶段回调具体 App 的 hook 契约。
- `Plugin`：可复用能力单元的标准生命周期契约，覆盖 init / input / update / resize / shutdown。
- `PluginInitCtx` / `PluginUpdateCtx` / `PluginRenderCtx` / `PluginResizeCtx` / `PluginShutdownCtx`：从 `RenderBackend*Ctx` 裁剪出的 plugin-facing context。
- `InputEvent`：平台输入事件的引擎侧表示

## 设计意图

- render loop 只依赖 `RenderApp`，因此平台层可以持有 `Box<dyn RenderApp>`，而不需要知道具体 App、Plugin 或 `RenderBackend`。
- 具体 App 通过 `RenderAppHooks` 暴露固定 hook 点，同时继续自行持有 GUI、camera/input state、overlay 和 render pipeline plugin。
- `RenderAppHooks::visit_plugins_mut` / `visit_plugins_mut_rev` 只暴露标准生命周期 Plugin，方便 runtime shell 批量调用 init / update / resize / shutdown。
- Plugin 的特有能力不放进统一 trait，例如 `GuiPlugin::ui`、`GuiPlugin::contribute_passes`、`RtPipeline::contribute_compute_passes` 仍由 App 通过具体类型显式调用。

## Ctx 边界

- `PluginInitCtx` 同时携带 `World`、`RenderWorld` 和初始化所需的 typed `Gfx` Ctx，用于创建 App/Plugin 持有的 GPU 资源。
- `PluginUpdateCtx` 面向 CPU 更新，提供 `World` 和帧设置相关状态，不承担 command recording。
- `PluginRenderCtx` 面向渲染录制，提供只读 `RenderWorld` 与 command/queue 相关能力，不包含 GUI draw data。
- `PluginResizeCtx` 和 `PluginShutdownCtx` 用于重建或释放 Plugin-owned GPU 资源，manager-owned image/view 必须通过 `RenderWorld` 中的 manager 释放。

## 边界约束

- 只定义契约，不依赖 `truvis-frame-runtime` 或具体 app
- `Plugin` trait 不包含 `ui`、`build_ui`、`contribute_passes` 等特有能力
- `RenderAppHooks::visit_plugins_mut` 只暴露标准生命周期 Plugin；特有能力仍通过具体 Plugin 类型显式调用
- GUI draw data 不进入通用 Plugin Ctx，由具体 `GuiPlugin` 自行管理
