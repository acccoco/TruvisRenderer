# Runtime / App / Subsystem 边界

> 状态：当前实现事实总结。本文记录状态所有权、`RenderAppRunner` 阶段边界、App hooks 与静态组合子系统的职责边界。

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

`RenderAppRunner` 不持有 ImGui、Camera、Overlay、InputState 或任何具体渲染子系统，也不访问 App 内部子系统。

具体 App state 持有：

- ImGui、camera/input、overlay 和 debug 选择等 CPU 交互状态；
- 具体渲染 subsystem 及其窗口尺寸资源、历史状态和 pass-local target；
- selection 等 App 业务语义，不保存由 runtime 私有 manager 分配的 GPU slot；
- Editor、desktop command 等只服务具体 App 的 controller；
- `ImGuiSubsystem`、`TriangleSubsystem`、`ShaderToySubsystem`、`RealtimeRenderSubsystem`、`OfflineRenderSubsystem`、
  `SelectionOutlineSubsystem` 和 `CoordinateGizmoSubsystem` 等具体能力。

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

`RenderAppRunner` 只将 init / resize / shutdown runtime ctx 包装为对应 `RenderApp*Ctx`，附带窗口尺寸或缩放因子；
update / after_prepare / render 直接传递对应 `RenderRuntime*Ctx`。具体 App 在生命周期 hook 内把
`&mut ctx.runtime` 直接交给自己持有的子系统，不重新构造等价的生命周期 ctx。

render hook 需要向通用渲染子系统缩窄能力时，通过 `app_kit::subsystem::SubsystemRenderCtx::from_runtime`
从 `RenderRuntimeRenderCtx` 创建只读视图。它包含 typed `Gfx` ctx、`RenderPassRecordCtx`、`RenderSceneView`、
`PresentView` 与 timeline，刻意不包含 `world_submesh_raster` 这类 App 专属能力。

这些 Ctx 携带 phase-appropriate 的 typed `Gfx` Ctx（如
device、resource、queue、surface、immediate、device-info），调用点只获得当前阶段需要的能力，不持有完整 `&Gfx`。

present owner 不直接暴露给 App / 子系统；render/init/resize ctx 只提供 `PresentView`。上层通过 `ImportedPresentTarget` 获取
RenderGraph 内的当前 present image 与 image info，acquire/render-complete semaphore 由
`PresentView::import_current_target` 固定接入 RenderGraph。

GUI draw data 不进入通用 ctx。`app-imgui::ImGuiSubsystem` 自行持有 imgui context、draw data、字体、GUI mesh 与私有 Vulkan backend；
App 在 update 中调用 `build_frame(delta, |ui| ...)`，在 render 中显式调用 `prepare_render_data` 与
`contribute_passes`，但不接触内部 GUI pass 或 GPU backend。App-owned debug 选择只保存稳定 CPU 语义；
`app-kit::DebugImageSelection` 不依赖 ImGui；对应渲染 subsystem 在 render phase 解析真实 image/view 与 layout。

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
- `on_input`
- `update`
- `after_prepare`
- `render`
- `render_view`
- `on_resize`
- `shutdown`

Runner 只调用这些 App 阶段，不遍历、注册或发现 App 内部对象。具体 App 在 `init`、`on_resize` 和 `shutdown`
中显式调用各子系统的生命周期，并自行保证初始化依赖、两条 resize 路径与 GPU 资源销毁顺序。

输入事件目前仍由 App hooks 显式处理，因为 GUI 事件消费和 App 自有 `InputManager` 之间存在 App 级策略。Editor request
与 Tauri desktop command 同样由 `TruvisRenderApp::update` 显式编排：它们需要访问 App 选择状态或权威 `World`，但没有标准
子系统生命周期或可复用 GPU 能力。

## 静态组合与 SubsystemLifecycle

具体 App 通过字段静态组合编译期已知的能力对象。只有确实拥有需要显式管理的长期资源的类型才实现
`app_kit::subsystem::SubsystemLifecycle`：

- `init`
- `on_resize`：可选默认空实现
- `shutdown`

`init` 与 `shutdown` 必须显式实现，分别接收 `RenderRuntimeInitCtx` 和 `RenderRuntimeShutdownCtx`；
可选 `on_resize` 接收 `RenderRuntimeResizeCtx`。trait 不承担注册、动态组合、事件派发、update 或 render 调度，
也不要求纯 CPU overlay、controller、camera 或 input object 实现生命周期。

每个子系统的具体能力保留在自身类型上，可跨越多个 render loop phase。例如：

- `ImGuiSubsystem::on_input` / `build_frame` / `prepare_render_data` / `contribute_passes`
- `TriangleSubsystem::contribute_passes`
- `ShaderToySubsystem::contribute_passes`
- `RealtimeRenderSubsystem::contribute_compute_passes` / `contribute_present_passes`
- `OfflineRenderSubsystem::contribute_compute_passes` / `contribute_present_passes`
- `SelectionOutlineSubsystem::contribute_passes`
- `CoordinateGizmoSubsystem::contribute_passes`

装配或拆卸一个子系统意味着修改具体 App 的字段及相关阶段调用，是编译期静态组合，不引入 visitor、
`dyn SubsystemLifecycle`、registry、downcast 或运行时安装机制。

## 边界不变量

- `RenderRuntime` 是 phase 能力来源，但不是 App / 子系统编排者。
- `RenderAppRunner` 是唯一固定帧骨架，独占 runtime，并通过内部 `dyn RenderApp` 回调具体业务阶段和裁剪 ctx。
- Runner 定义阶段边界；App 编排阶段内部；子系统只实现自己的具体能力。
- App 是业务组合 owner，持有具体子系统，并在 render 阶段决定 RenderGraph pass 顺序与 realtime/offline 分支。
- `app-kit` 不依赖具体 subsystem；ImGui 与 rendering 分属独立 crate，设置 UI 只作为两者之间的集成层。
- Tauri 文件对话框和私有 desktop command bridge 属于具体 App 的平台特权能力；本地路径不得进入 Editor WebSocket、
  `RenderRuntime` 或通用生命周期接口，Tauri main thread 也不得直接修改 `World`。
- `SubsystemLifecycle` 只规范 init / resize / shutdown；特有能力和调用顺序始终由具体 App 显式控制。
- App / 子系统不长期保存完整 runtime owner、typed `Gfx` ctx 或底层 Vulkan/VMA 依赖。
