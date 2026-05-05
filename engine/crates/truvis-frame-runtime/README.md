# truvis-frame-runtime

Frame runtime crate 提供 `BaseApp` 帧骨架与 `FrameAppShell` 适配层。

## 主要职责

- 持有 `RenderBackend` 和待处理 `InputEvent` 队列
- 固定执行 `begin_frame -> input -> update -> prepare -> render -> present -> end_frame`
- 通过 `FrameAppHooks` 在 input / update / render / camera 位置回调具体 App
- 通过 `FrameAppShell<A>` 把 `A: FrameAppState` 包装成 render loop 可驱动的 `FrameApp`
- 提供 `init_env` 初始化日志、panic hook 和 tracy client

## 边界约束

- `BaseApp` 不持有 GUI、Camera、Overlay、InputState 或具体 render pipeline plugin
- `FrameAppShell` 持有 `BaseApp` 和具体 App state，具体 App state 不直接持有 `BaseApp`
- resize 只调用 RenderBackend 并把 `RenderBackendResizeCtx` 通过 `FrameAppResizeCtx` 返回给 App state，由 App state 决定通知哪些 Plugin
- Vulkan 资源销毁顺序为 App state 先 shutdown plugins，再调用 `BaseApp::destroy`
