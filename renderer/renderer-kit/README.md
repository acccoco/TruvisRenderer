# renderer-kit

`renderer-kit` 是 Renderer 域的基础能力层，只保存生命周期契约、相机/输入和与具体界面、渲染实现无关的纯 CPU 状态。

## 主要职责

- `SubsystemLifecycle`：只约束需要显式资源管理的子系统 `init` / 可选 `on_resize` / `shutdown`；具体 Renderer 静态持有并直接调用。
- `SubsystemRenderCtx`：从 `RenderRuntimeRenderCtx` 裁剪出通用渲染子系统所需的只读能力，不扩散 `world_submesh_raster` 等产品专属接口。
- `Camera` / `CameraController` / `InputManager` / `InputState`：Renderer-owned 相机与输入状态，生成 runtime prepare 所需的 `RenderView` 快照。
- `DebugImageOption` / `DebugImageSelection`：稳定候选 ID、标签、显示开关与当前选择；不依赖 ImGui，不保存 GPU image/view 或 RenderGraph handle。

## 所有权与依赖

- 不依赖 `imgui`、`renderer-render-passes`、`truvis-renderer-shader-binding` 或任何具体渲染 subsystem。
- 不拥有具体 Renderer state、GUI backend、render controls、GPU pass、realtime/offline targets，也不提供可执行入口。
- `DebugImageSelection::selected_id()` 在可见性关闭时返回 `None`；`normalize_options()` 在模式切换或窗口隐藏时仍可由 Renderer 显式调用。
- `renderer-imgui` 持有 ImGui context、字体、backend 与选择器视图；`renderer-rendering` 持有具体 rendering subsystem 及其长期 GPU 资源。
- 纯 UI overlay、camera 和 input 不需要实现 `SubsystemLifecycle`；生命周期 trait 不引入 registry、visitor、动态分发或自动调度。
- 相机状态属于 Renderer；runtime 只消费 `RenderView`，不依赖相机控制策略。中键 pivot、Shift+中键拖拽与滚轮锚点状态留在 `CameraController`，同步 raycast 仍由具体 Renderer 在 `after_prepare` 阶段执行。

跨 crate 所有权和帧阶段见 [`docs/summaries/runtime-renderer-subsystem-boundaries.md`](../../docs/summaries/runtime-renderer-subsystem-boundaries.md)。
