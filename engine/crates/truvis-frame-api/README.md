# truvis-frame-api

Frame API crate 定义 app 和 plugin 的稳定契约，不持有运行时状态。

## 主要职责

- `RenderApp`：render loop 看到的对象安全 App 契约，通常由 `truvis-frame-runtime::RenderAppShell` 实现
- `RenderAppHooks`：`RenderAppShell` 在生命周期和帧骨架内回调 App 的 hook 点，并通过 visitor 获取 App 持有的标准生命周期 Plugin
- `Plugin`：可复用能力单元的标准生命周期
- `PluginInitCtx` / `PluginUpdateCtx` / `PluginRenderCtx` / `PluginResizeCtx`：从 RenderBackend Ctx 裁剪出的 plugin-facing context
- `InputEvent`：平台输入事件的引擎侧表示

## 边界约束

- 只定义契约，不依赖 `truvis-frame-runtime` 或具体 app
- `Plugin` trait 不包含 `ui`、`build_ui`、`contribute_passes` 等特有能力
- `RenderAppHooks::visit_plugins_mut` 只暴露标准生命周期 Plugin；特有能力仍通过具体 Plugin 类型显式调用
- GUI draw data 不进入通用 Plugin Ctx，由具体 `GuiPlugin` 自行管理
