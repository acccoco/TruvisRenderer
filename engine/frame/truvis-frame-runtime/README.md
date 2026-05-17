# truvis-frame-runtime

Frame runtime crate 提供 `RenderAppShell` 帧骨架与 render-loop 适配层。它是固定帧顺序的唯一实现位置，但不拥有具体 App 业务状态。

## 主要职责

- 持有 `RenderBackend` 和待处理 `InputEvent` 队列
- 固定执行 `begin_frame -> input -> update -> plugin update -> prepare -> render -> present -> end_frame`
- 通过 `RenderAppHooks` 在 init / input / update / render / resize / shutdown 位置回调具体 App
- 通过 `RenderAppHooks::visit_plugins_mut` 批量调用 App 持有 Plugin 的 init / update / resize / shutdown 标准生命周期
- 通过 `RenderAppShell<A>` 把 `A: RenderAppHooks` 包装成 render loop 可驱动的 `RenderApp`
- 提供 `init_env` 初始化日志、panic hook 和 tracy client

## 帧骨架

`RenderAppShell::run_frame` 负责稳定的每帧顺序：

1. `RenderBackend::begin_frame`
2. drain `InputEvent` 并调用 `RenderAppHooks::on_input`
3. `RenderBackend::update_phase` 后调用 `RenderAppHooks::update`
4. 通过 `visit_plugins_mut` 调用 `Plugin::update`
5. `RenderBackend::prepare(app.camera())`
6. `RenderBackend::render_phase` 后调用 `RenderAppHooks::render`
7. `RenderBackend::present`
8. `RenderBackend::end_frame`

这个顺序是 runtime 层的核心设计约束。App 和 Plugin 可以决定具体业务行为，但不重复实现通用 frame skeleton。

## 边界约束

- `RenderAppShell` 不持有 GUI、Camera、Overlay、InputState 或具体 render pipeline plugin
- `RenderAppShell` 持有 `RenderBackend`、待处理输入事件队列和具体 App hooks
- resize 只调用 RenderBackend 并把 `RenderBackendResizeCtx` 通过 `RenderAppResizeCtx` 返回给 App hooks，再按 App 提供的 visitor 顺序通知 Plugin
- shutdown 时先等待 GPU idle，再将 `RenderAppShutdownCtx` 交给 App hooks，并将 `PluginShutdownCtx` 交给 Plugin shutdown
- App / Plugin 持有的 GPU 资源必须在 shutdown context 中释放；需要 manager-owned image/view 或 bindless 注销时使用 context 中的 `RenderWorld`
- Vulkan 资源销毁顺序为 App hooks shutdown、Plugin shutdown，再销毁 RenderBackend 和 `Gfx`

## 设计意图

- render loop 面向 `Box<dyn RenderApp>`，因此平台层只负责驱动生命周期，不直接接触 backend 字段。
- `RenderAppShell` 合并了 runtime shell 需要的基础设施：`RenderBackend`、输入队列和固定帧顺序。
- 具体 App 继续拥有 GUI、camera/input state、overlay 和 render pipeline plugin，避免 runtime shell 反向依赖应用集成层。
- resize 采用条件通知：只有 `RenderBackend::handle_resize` 实际重建 swapchain 资源并返回 `Some(RenderBackendResizeCtx)` 时，才通知 App 和 Plugin。
