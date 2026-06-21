# Runtime / App / Plugin 边界

> 状态：当前实现事实总结。本文记录状态所有权、`RenderAppShell` 适配层、App hooks 与 Plugin 的职责边界。

## 状态所有权

`RenderRuntime` 持有渲染运行时核心状态：

```text
RenderRuntime
  -> Gfx         Vulkan root owner + typed Ctx factory
  -> World       CPU scene + assets
  -> GfxResourceManager manager-owned GPU image/buffer/view
  -> ShaderBindingSystem global descriptors + bindless + sampler
  -> FrameTiming frame counter + delta/total time
  -> PerFrameGpuData per-FIF PerFrameData UBO
  -> FrameRenderState / DlssOptions / ViewAccumState / DlssSrState runtime render state
  -> GpuScene    runtime 私有 GPU scene buffer / TLAS / raster draw cache
  -> RayCastService prepare 后同步 raycast 的 runtime-owned pipeline / buffer / fence
  -> SwapchainPresenter swapchain/present resources
```

`RenderAppShell` 只持有：

- `RenderRuntime`
- 待处理 `InputEvent` 队列
- 具体 App hooks

`RenderAppShell` 不持有 GUI、Camera、Overlay、InputState 或任何具体 render pipeline plugin。

具体 App state 持有：

- `app_kit::GuiPlugin`
- `CameraController` / `InputManager`
- App 自有 overlay 编排器，或 app-kit 提供的 `DebugInfoOverlay` / `PipelineControlsOverlay`
  兼容整窗 wrapper；Truvis 使用 `TruvisOverlayUi` 统一决定 tag、窗口布局、section 可见性和绘制顺序。
  默认布局保留透明 diagnostics HUD，将 Rendering controls 与 Picking 结果上下拼接到左侧主面板，
  Debug Images 由 `GuiPlugin` 继续持有状态，并作为独立小窗口锚定到 swapchain 右侧。
- `TrianglePlugin`、`ShaderToyPlugin`、`RtPipeline`、`OfflinePipeline` 等具体渲染能力

## Ctx 裁剪契约

RenderRuntime 通过 lifecycle Ctx 借出内部字段：

- `RenderRuntimeInitCtx`
- `RenderRuntimeUpdateCtx`
- `RenderRuntimeRayCastCtx`
- `RenderRuntimeRenderCtx`
- `RenderRuntimeResizeCtx`
- `RenderRuntimeShutdownCtx`

`RenderAppShell` 从 RenderRuntime Ctx 裁剪标准生命周期需要的 Plugin Ctx，App 在 render hook 中为特有 render 能力裁剪
`PluginRenderCtx`：

- `PluginInitCtx`
- `PluginUpdateCtx`
- `PluginRenderCtx`
- `PluginResizeCtx`
- `PluginShutdownCtx`

这些 Ctx 携带 phase-appropriate 的 typed `Gfx` Ctx（如
device、resource、queue、surface、immediate、device-info），调用点只获得当前阶段需要的能力，不持有完整 `&Gfx`。

present owner 不直接暴露给 app/plugin；render/init/resize Ctx 只提供 `PresentView`。上层通过 `ImportedPresentTarget` 获取
RenderGraph 内的当前 present image 与 image info，acquire/render-complete semaphore 由
`PresentView::import_current_target` 固定接入 RenderGraph。

GUI draw data 不进入通用 Ctx。`GuiPlugin` 自行持有 imgui context、draw data、GUI mesh buffer、font texture map，
并通过 `prepare_render_data` 和 `contribute_passes` 接入 render hook。Debug Images 的窗口外壳可由具体 App
重新编排，但选择状态、texture id 映射和每帧 image/view handle 快照仍归 `GuiPlugin`。

## RenderApp 外部契约

`RenderApp` 是 render loop 的外部契约：

- `init_after_window`
- `run_frame`
- `push_input_event`
- `recreate_swapchain_if_needed`
- `time_to_render`
- `shutdown`

`RenderAppShell<A>` 是适配层：它实现 `RenderApp`，持有 `RenderRuntime`、输入事件队列和 `A: RenderAppHooks`，把 render loop
的外部生命周期转发到 runtime 与具体 App hooks。

## RenderAppHooks 契约

`RenderAppHooks` 是 `RenderAppShell` 回调具体 App 的 hook 契约：

- `init`
- `visit_plugins_mut`
- `visit_plugins_mut_rev`
- `on_input`
- `update`
- `after_prepare`
- `render`
- `camera`
- `on_resize`
- `shutdown`

`RenderAppShell` 使用 `visit_plugins_mut` 批量调用 `Plugin::init`、`Plugin::update` 和 `Plugin::on_resize`，使用
`visit_plugins_mut_rev` 调用 `Plugin::shutdown`。

输入事件目前仍由 App hooks 显式处理，因为 GUI 事件消费和 App 自有 `InputManager` 之间存在 App 级策略。

## Plugin 模型

`Plugin` 是可复用能力单元的标准生命周期：

- `init`
- `on_input`
- `update`
- `on_resize`
- `shutdown`

Plugin 的特有能力不放进统一 trait。例如：

- `GuiPlugin::begin_frame` / `ui` / `end_frame` / `prepare_render_data` / `contribute_passes`
- `TrianglePlugin::contribute_passes`
- `ShaderToyPlugin::contribute_passes`
- `RtPipeline::contribute_compute_passes` / `contribute_present_passes`
- `OfflinePipeline::contribute_compute_passes` / `contribute_present_passes`

App 通过持有具体类型来组合这些能力，并通过 visitor 暴露标准生命周期 Plugin，不使用 downcast、注册表或消息总线。

## 边界不变量

- `RenderRuntime` 是 phase 能力来源，但不是 App / Plugin 编排者。
- `RenderAppShell` 是固定帧骨架，只转发外部生命周期并裁剪 ctx。
- App 是业务组合 owner，持有具体 Plugin，并在 render 阶段决定 RenderGraph pass 顺序；Truvis 也在这里按 `RenderMode` 选择实时或离线 sub RenderGraph。
- Plugin 是可复用能力单元；标准生命周期可以批量驱动，特有能力由 App 显式调用。
- App / Plugin 不长期保存完整 runtime owner、typed `Gfx` Ctx 或底层 Vulkan/VMA 依赖。
