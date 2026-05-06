# truvis-frame-runtime

Frame runtime crate 提供 `RenderAppShell` 帧骨架与 render-loop 适配层。

## 主要职责

- 持有 `RenderBackend` 和待处理 `InputEvent` 队列
- 固定执行 `begin_frame -> input -> update -> plugin update -> prepare -> render -> present -> end_frame`
- 通过 `RenderAppHooks` 在 init / input / update / render / resize / shutdown 位置回调具体 App
- 通过 `RenderAppHooks::visit_plugins_mut` 批量调用 App 持有 Plugin 的 init / update / resize / shutdown 标准生命周期
- 通过 `RenderAppShell<A>` 把 `A: RenderAppHooks` 包装成 render loop 可驱动的 `RenderApp`
- 提供 `init_env` 初始化日志、panic hook 和 tracy client

## 边界约束

- `RenderAppShell` 不持有 GUI、Camera、Overlay、InputState 或具体 render pipeline plugin
- `RenderAppShell` 持有 `RenderBackend`、待处理输入事件队列和具体 App hooks
- resize 只调用 RenderBackend 并把 `RenderBackendResizeCtx` 通过 `RenderAppResizeCtx` 返回给 App hooks，再按 App 提供的 visitor 顺序通知 Plugin
- Vulkan 资源销毁顺序为 App hooks shutdown、Plugin shutdown，再销毁 RenderBackend 和 `Gfx`
