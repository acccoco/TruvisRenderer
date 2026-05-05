# ARCHITECTURE.md

本文档记录项目的总体架构、设计思路与模块约束。具体 crate 入口、文件职责和运行命令请查看对应模块 README。

## 1. 分层与依赖边界

项目目标是保持无环依赖：上层可以依赖下层，下层不反向依赖上层业务。

```text
L6  truvis-winit-app
    winit 事件循环、窗口生命周期、渲染线程启动

L5  truvis-app
    demo app、GuiPlugin、overlay plugin、render pipeline plugin、RenderGraph 编排

L5  truvis-frame-runtime
    BaseApp 帧骨架：Renderer 生命周期 + 输入事件队列

L5  truvis-frame-api
    FrameApp / FrameAppHooks / Plugin 契约与 Plugin Ctx

L4  truvis-renderer
    Renderer：World + RenderWorld + swapchain/present/cmd/sync 生命周期

L3  truvis-render-graph / truvis-scene / truvis-asset / truvis-gui-backend
    图调度、CPU 场景、资产加载、底层 ImGui Vulkan 录制

L2  truvis-render-interface
    RenderWorld、BindlessManager、GpuScene、FrameCounter、CmdAllocator、FifBuffers

L1  truvis-gfx
    Vulkan RHI 封装

L0  truvis-utils / truvis-logs / truvis-path / descriptor-layout
```

GUI 的 RenderGraph 适配位于 `truvis-app::gui_plugin`，底层 `truvis-gui-backend` 只保留 `GuiMesh` / `GuiPass` 等 Vulkan 后端能力，不依赖 render graph 或 frame runtime。

## 2. 生命周期

启动入口唯一：平台层创建窗口和渲染线程，渲染线程只通过 `Box<dyn FrameApp>` 驱动 App。

启动流程：

```text
WinitApp::run_app
  -> init_env
  -> create Window + SharedState
  -> spawn RenderThread
  -> app_factory() -> Box<dyn FrameApp>
  -> app.init_after_window(raw handles, scale_factor, initial_size)
```

每帧流程：

```text
render_loop
  -> drain channel InputEvent
  -> app.push_input_event(event)
  -> read latest window size
  -> app.recreate_swapchain_if_needed(size)
  -> app.time_to_render()
  -> app.run_frame()
```

`FrameApp::run_frame` 由具体 App 通过 `Option<BaseApp>::take()` 取出 `BaseApp`，调用 `base.run_frame(self)` 后放回，避免同时可变借用 `self.base` 和 `self` 的 hook 字段。

`BaseApp::run_frame` 的固定顺序：

```text
renderer.begin_frame()
drain input events -> app.on_input(events)
{ update_ctx = renderer.update_phase(); app.update(&mut update_ctx); }
renderer.prepare(app.camera())
{ render_ctx = renderer.render_phase(); app.render(&render_ctx); }
renderer.present()
renderer.end_frame()
```

关闭流程：

- 渲染线程观察到退出信号后调用 `FrameApp::shutdown(&mut self)`。
- App 先 shutdown 自己持有的 Plugin，再 `take()` 出 `BaseApp` 调用 `BaseApp::destroy()`。
- `BaseApp::destroy()` 等待 GPU idle，销毁 Renderer，再销毁 `Gfx`。
- 主线程等待渲染线程完成后再 drop `Window`。

## 3. 状态所有权

`Renderer` 持有渲染后端核心状态：

```text
Renderer
  -> World       CPU scene + assets
  -> RenderWorld GPU resources + frame state
  -> RenderPresent swapchain/present resources
```

`BaseApp` 只持有：

- `Renderer`
- 待处理 `InputEvent` 队列

`BaseApp` 不持有 GUI、Camera、Overlay、InputState 或任何具体 render pipeline plugin。

具体 App 持有：

- `Option<BaseApp>`
- `GuiPlugin`
- `CameraController` / `InputManager`
- `DebugInfoOverlay` / `PipelineControlsOverlay`
- `TrianglePlugin`、`ShaderToyPlugin` 或 `RtPipeline` 等具体渲染能力

Renderer 通过 lifecycle Ctx 借出内部字段：

- `RendererInitCtx`
- `RendererUpdateCtx`
- `RendererRenderCtx`
- `RendererResizeCtx`

App 在 hook 中从 Renderer Ctx 裁剪出 Plugin Ctx：

- `PluginInitCtx`
- `PluginUpdateCtx`
- `PluginRenderCtx`
- `PluginResizeCtx`

GUI draw data 不进入通用 Ctx。`GuiPlugin` 自行持有 imgui context、draw data、GUI mesh buffer、font texture map，并通过 `prepare_render_data` 和 `contribute_passes` 接入 render hook。

## 4. App / BaseApp / Plugin 模型

`FrameApp` 是 render loop 的外部契约：

- `init_after_window`
- `run_frame`
- `push_input_event`
- `recreate_swapchain_if_needed`
- `time_to_render`
- `shutdown`

`FrameAppHooks` 是 `BaseApp` 的内部回调契约：

- `on_input`
- `update`
- `render`
- `camera`

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

App 通过持有具体类型来组合这些能力，不使用 downcast、注册表或消息总线。

## 5. RenderGraph 与数据流

CPU 语义数据从 `World` 进入 Renderer prepare 阶段，再同步到 GPU 可见资源：

```text
AssetHub / SceneManager
  -> Renderer::prepare(camera)
  -> GpuScene / BindlessManager / GlobalDescriptorSets
  -> App render hook builds RenderGraph
  -> pass command recording
  -> queue submit
  -> swapchain present
```

RenderGraph 规则：

- App 在 `FrameAppHooks::render` 中创建 RenderGraph。
- 渲染管线 Plugin 只贡献自己的 pass，不决定整个 App 的拓扑。
- App 显式决定 GUI pass 与渲染管线 pass 的顺序。
- pass 必须声明资源读写关系，让 RenderGraph 推导同步。

Triangle / ShaderToy 使用单个 present graph。RT demo 使用 compute graph 与 present graph：App 先让 `RtPipeline` 贡献 compute passes，再在 present graph 中先 resolve，最后调用 `GuiPlugin::contribute_passes` 叠加 GUI。

## 6. 线程与同步

线程模型：

```text
main thread
  owns EventLoop and Window
  sends InputEvent through channel
  writes latest size to AtomicU64
  sets exit flag

render thread
  owns Box<dyn FrameApp>
  owns BaseApp/Renderer through App
  creates, uses, destroys all Vulkan objects
```

约束：

- 主线程不调用 Vulkan、`ash` 或 `truvis-gfx` API。
- 所有 Vulkan 对象在渲染线程创建、使用和销毁。
- resize 通过 latest-size 模式合并连续事件。
- GPU 同步优先通过 RenderGraph、binary semaphore 和 frame timeline 表达。

## 7. 资源生命周期

资源分类：

- Persistent：pipeline、sampler、descriptor layout、shader binding
- Frame：command buffer、per-frame buffer、FIF resources
- Swapchain：swapchain image/view、present semaphore、window-sized targets
- Asset：texture、mesh buffer、material-related GPU resources
- GUI：imgui font texture、per-frame GUI mesh buffer、texture map
- RenderGraph transient：图内临时 image/buffer

创建路径：

- `Renderer::new` 初始化 `Gfx`，创建 `World` / `RenderWorld`。
- `Renderer::init_after_window` 创建 surface、swapchain 和 `RenderPresent`。
- App 从 `RendererInitCtx` 构造 `PluginInitCtx`，依次初始化自己持有的 Plugin。

重建路径：

- render loop 调用 `FrameApp::recreate_swapchain_if_needed(size)`。
- App 委托 `BaseApp::recreate_swapchain_if_needed(size)`。
- Renderer 只有实际重建时返回 `Some(RendererResizeCtx)`。
- App 构造 `PluginResizeCtx` 并通知需要 resize 的 Plugin。

销毁路径：

- `FrameApp::shutdown(&mut self)`：Plugin shutdown -> `BaseApp::destroy()`。
- `BaseApp::destroy()`：`Gfx::wait_idle()` -> `renderer.destroy()` -> `Gfx::destroy()`。
