# truvis-frame-api

Frame API crate 定义 app 和 plugin 的稳定契约，不持有运行时状态。

## 主要职责

- `FrameApp`：render loop 看到的对象安全 App 契约
- `FrameAppHooks`：`BaseApp` 在帧骨架内回调 App 的 hook 点
- `Plugin`：可复用能力单元的标准生命周期
- `PluginInitCtx` / `PluginUpdateCtx` / `PluginRenderCtx` / `PluginResizeCtx`：App 从 RenderBackend Ctx 裁剪出的 plugin-facing context
- `InputEvent`：平台输入事件的引擎侧表示

## 边界约束

- 只定义契约，不依赖 `truvis-frame-runtime` 或具体 app
- `Plugin` trait 不包含 `ui`、`build_ui`、`contribute_passes` 等特有能力
- GUI draw data 不进入通用 Plugin Ctx，由具体 `GuiPlugin` 自行管理
