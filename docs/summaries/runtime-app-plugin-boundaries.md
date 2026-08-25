# Runtime / App / Plugin 边界

> 状态：当前实现事实总结。本文记录状态所有权、`RenderAppRunner` 适配层、App hooks 与 Plugin 的职责边界。

## 状态所有权

窗口、线程、执行器与资源的唯一所有权链为：

```text
Window Host
  owns Window + backend-independent RenderThread handle

OS RenderThread
  owns RenderAppRunner

RenderAppRunner
  owns Box<dyn RenderApp> + RenderRuntime

RenderRuntime
  owns World + Gfx + GPU resources
```

`RenderRuntime` 持有渲染运行时核心状态：

```text
RenderRuntime
  -> Gfx         Vulkan root owner + typed Ctx factory
  -> World       CPU scene + assets
  -> GfxResourceManager manager-owned GPU image/buffer/view
  -> ShaderBindingSystem global descriptors + bindless + sampler
  -> FrameTiming frame id + delta/total time + default 120 FPS minimum frame interval
  -> PerFrameGpuData per-FIF PerFrameData UBO
  -> FrameRenderState / DlssOptions / ViewAccumState / DlssSrState runtime render state
  -> RenderWorld runtime 私有 render managers / GPU scene buffer / raster draw cache / RenderTlasManager
  -> RayCastService prepare 后同步 raycast 的 runtime-owned pipeline / buffer / fence
  -> SwapchainPresenter swapchain/present resources
```

`RenderAppRunner` 只持有：

- `RenderRuntime`
- 待处理 `InputEvent` 队列
- 一次性构造注入的 `Box<dyn RenderApp>`

`RenderAppRunner` 不持有 GUI、Camera、Overlay、InputState 或任何具体 render pipeline plugin。

具体 App state 持有：

- GUI、camera/input、overlay 和 debug 选择等 CPU 交互状态；
- 具体 render pipeline/plugin 及其窗口尺寸资源、历史状态和 pass-local target；
- selection 等 App 业务语义，不保存由 runtime 私有 manager 分配的 GPU slot；
- Editor、desktop command 等只服务具体 App 的 controller；
- `TrianglePlugin`、`ShaderToyPlugin`、`RtPipeline`、`OfflinePipeline` 等具体渲染能力。

主体 Truvis App 的具体组合、UI/selection owner 和 pass 顺序见 [`app/truvis/README.md`](../../app/truvis/README.md)；
Editor 协议与线程边界见 [`editor-subsystem.md`](editor-subsystem.md)。

## Ctx 裁剪契约

RenderRuntime 通过 lifecycle Ctx 借出内部字段：

- `RenderRuntimeInitCtx`
- `RenderRuntimeUpdateCtx`
- `RenderRuntimeRayCastCtx`
- `RenderRuntimeRenderCtx`
- `RenderRuntimeResizeCtx`
- `RenderRuntimeShutdownCtx`

`RenderAppRunner` 从 RenderRuntime Ctx 裁剪标准生命周期需要的 Plugin Ctx，App 在 render hook 中为特有 render 能力裁剪
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

GUI draw data 不进入通用 Ctx。`GuiPlugin` 自行持有 imgui context、draw data 和 GUI GPU 资源，
并通过自己的 prepare/contribute 接口接入 render hook。App-owned debug 选择只保存稳定 CPU 语义；
当前 pipeline 在 render phase 解析真实 image/view 与 layout。

selection outline 使用单独的 `WorldSubmeshRasterView` render ctx 能力。该能力只接受
`WorldSubmeshSelection`，由 runtime 在当前 prepare 快照内解析 active instance slot 与 draw cache；App 不接触
`RenderWorld` concrete owner，也不把 outline mask 暴露为可选 debug image source。

## RenderAppRunner 外部契约

`RenderAppRunner` 是平台无关的完整生命周期 owner，对平台层只公开一个运行入口：

```rust
RenderAppRunner::run(
    control: Arc<RenderThreadControl>,
    init: RenderThreadInit,
    app: Box<dyn RenderApp>,
)
```

这个非泛型执行器在内部独占 `RenderRuntime`、输入事件队列和具体 App，统一驱动窗口绑定初始化、输入、resize、每帧阶段和 shutdown。
`RenderThreadControl` 只封装 Runner 消费的退出状态、无界输入队列，以及 latest-size 和 resize generation；线程完成标记、
panic 传播与 join 归属于 `truvis-render-thread::RenderThread`。`RenderThreadInit` 只携带 raw window/display handle、
scale factor 和初始尺寸。winit backend 只跨线程传递可 Send 的 `Win32WindowHandle` / `WindowsDisplayHandle`，
在目标 OS RenderThread 内重建 raw enum、执行 App factory，并进入统一 Runner，因此 standalone 与 embedded 入口共享同一帧骨架。

## RenderApp 契约

`RenderApp` 是 `RenderAppRunner` 内部持有的 object-safe 具体 App 契约：

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

`RenderAppRunner` 使用 `visit_plugins_mut` 批量调用 `Plugin::init`、`Plugin::update` 和 `Plugin::on_resize`，使用
`visit_plugins_mut_rev` 调用 `Plugin::shutdown`。

输入事件目前仍由 App hooks 显式处理，因为 GUI 事件消费和 App 自有 `InputManager` 之间存在 App 级策略。Editor request
与 Tauri desktop command 同样由 `TruvisRenderApp::update` 显式编排：它们需要访问 App 选择状态或权威 `World`，但没有标准
Plugin 生命周期或可复用 GPU 能力。

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
- `RenderAppRunner` 是唯一固定帧骨架，独占 runtime，并通过内部 `dyn RenderApp` 回调具体业务阶段和裁剪 ctx。
- App 是业务组合 owner，持有具体 Plugin，并在 render 阶段决定 RenderGraph pass 顺序与具体 pipeline 分支。
- Tauri 文件对话框和私有 desktop command bridge 属于具体 App 的平台特权能力；本地路径不得进入 Editor WebSocket、
  `RenderRuntime` 或通用 Plugin 接口，Tauri main thread 也不得直接修改 `World`。
- Plugin 是可复用能力单元；标准生命周期可以批量驱动，特有能力由 App 显式调用。
- App / Plugin 不长期保存完整 runtime owner、typed `Gfx` Ctx 或底层 Vulkan/VMA 依赖。
