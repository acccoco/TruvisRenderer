# ARCHITECTURE.md

本文档从架构需要关注的六个维度描述项目：

- 分层边界
- 生命周期
- 状态所有权
- 数据流
- 调度同步
- 资源生命周期。

它不替代模块 README；具体 crate 入口、文件职责和运行命令请查看对应模块文档。

## 1. 分层与依赖边界

分层的核心目的不是整理目录，而是隔离变化原因。

平台事件循环、应用扩展契约、渲染后端、领域数据、渲染契约和 Vulkan RHI 的变化频率不同，必须放在不同层级中。

项目目标是保持**无环依赖**：上层可以依赖下层，下层不反向依赖上层业务。

```text
┌──────────────────────────────────────────────────────────────────────┐
│  L6  Platform Entry                                                  │
│  ┌──────────────────┐                                                │
│  │ truvis-winit-app │  winit 事件循环 + 渲染线程分离                  │
│  └────────┬─────────┘                                                │
│           │                                                          │
│  L5  App Contract + Runtime                                          │
│  ┌────────────────┐  ┌────────────────────┐  ┌───────────────────┐   │
│  │  truvis-app    │  │ truvis-frame-      │  │ truvis-render-    │   │
│  │  (demo apps)   │  │ runtime            │  │ passes            │   │
│  └───────┬────────┘  └────────┬───────────┘  └───────┬───────────┘   │
│          │                    │                      │               │
│          │           ┌───────┴──────────┐            │               │
│          │           │  truvis-app-api  │            │               │
│          │           │  (plugin 契约)    │            │               │
│          │           └───────┬──────────┘            │               │
│           \                  │                      /                │
│  L4  Renderer Integration    │                     /                 │
│  ┌───────────────────────────┴────────────────────┐                  │
│  │              truvis-renderer                   │                  │
│  │     持有 World (CPU) + RenderWorld (GPU)        │                  │
│  └───────┬────────────┬────────────┬──────────────┘                  │
│          │            │            │                                 │
│  L3  Domain + Graph (同层互不依赖)                                    │
│  ┌──────────────┐  ┌──────────┐  ┌──────────────┐  ┌──────────┐     │
│  │ render-graph │  │  scene   │  │    asset     │  │gui-backend│     │
│  │  (DAG 编排)  │  │ (CPU 场景)│  │ (异步加载)   │  │ (imgui)  │     │
│  └──────┬───────┘  └────┬─────┘  └──────┬───────┘  └──────────┘     │
│         │               │               │                           │
│         │        ┌──────┴───────┐        │                           │
│         │        │ truvis-world │        │                           │
│         │        │(CPU 聚合容器) │        │                           │
│         │        └──────────────┘        │                           │
│         │                                │                           │
│  L2  Render Contract                     │                           │
│  ┌───────────────────────────────────────┴───────┐                   │
│  │          truvis-render-interface              │                   │
│  │  RenderWorld / BindlessManager / GpuScene     │                   │
│  │  FrameCounter / CmdAllocator / FifBuffers     │                   │
│  └───────────────────┬───────────────────────────┘                   │
│                      │                                               │
│  L1  RHI             │                                               │
│  ┌───────────────────┴──────────────────┐                            │
│  │            truvis-gfx                │                            │
│  │   Vulkan 封装 (ash): Device,Queue,   │                            │
│  │   Pipeline,Image,Buffer,Barrier...   │                            │
│  └──────────────────────────────────────┘                            │
│                                                                      │
│  L0  Foundation                                                      │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────────────┐     │
│  │  utils   │ │   logs   │ │   path   │ │ descriptor-layout    │     │
│  └──────────┘ └──────────┘ └──────────┘ │ (macro + trait)      │     │
│                                         └──────────────────────┘     │
└──────────────────────────────────────────────────────────────────────┘
```

如果新增功能需要跨层依赖，应先判断这个功能的稳定职责属于哪一层，而不是为了调用方便直接引入反向依赖。

## 2. 时序与生命周期

生命周期描述系统在时间上如何启动、推进、重建和关闭。

架构上最重要的是入口唯一：启动由平台层触发，帧推进由 `FrameRuntime` 负责，swapchain 重建走 runtime 单入口，关闭通过主线程和渲染线程二阶段握手完成。

启动生命周期：

- `WinitApp::run_plugin` 初始化环境，创建 winit `EventLoop`。
- winit `resumed` 后创建 `Window`，准备 `SharedState` 和 raw window/display handle。
- 主线程 spawn `RenderThread`，渲染线程内部创建 `FrameRuntime`。
- `Renderer::new` 初始化 `Gfx`，创建 `World` 和 `RenderWorld`。
- `init_after_window` 创建 surface、swapchain 和 `RenderPresent`。
- `AppPlugin::init(InitCtx)` 做一次性应用初始化，随后注册 GUI font 资源。

每帧生命周期：

```text
render_loop
  -> drain input events
  -> read latest window size
  -> recreate_swapchain_if_needed
  -> time_to_render
  -> FrameRuntime::run_frame
```

`FrameRuntime::run_frame` 内部阶段固定为：

- `begin_frame`：等待 frames-in-flight timeline，清理帧资源，推进 asset 更新。
- `phase_input`：GUI 先处理输入事件，随后 `InputManager` 更新输入状态。
- `phase_update`：acquire swapchain image，构建 UI，准备 GUI draw data，调用 `plugin.update`。
- `phase_prepare`：更新累积帧状态，将 CPU scene 上传到 GPU scene，更新 per-frame descriptors。
- `phase_render`：构造 `RenderCtx`，调用 `plugin.render`，应用侧构建并执行 RenderGraph。
- `phase_present`：present 当前 swapchain image，推进 `FrameCounter`。

每帧生命周期的精确展开：

```text
run_frame()
  ├─ begin_frame()
  │    renderer.begin_frame()      // wait GPU, reset FIF resources
  │    renderer.update_assets()    // AssetHub CPU tick
  │
  ├─ phase_input()
  │    input_manager.get_events()  // drain queued events
  │    gui_host.handle_event()     // forward to imgui
  │    input_manager.process()     // update input state
  │
  ├─ phase_update()
  │    renderer.update_frame_settings()
  │    renderer.acquire_image()    // vkAcquireNextImageKHR
  │    build_ui()                  // overlays + plugin.build_ui
  │    gui_host.compile_ui()       // imgui draw lists
  │    gui.prepare_render_data()   // upload imgui vertex/index
  │    camera_controller.update()
  │    plugin.update(UpdateCtx)    // CPU scene logic
  │
  ├─ phase_prepare()
  │    renderer.update_accum_frames()
  │    renderer.before_render()
  │      ├─ update_gpu_scene()     // scene → GPU buffer upload
  │      └─ update_perframe_desc() // write descriptor sets
  │
  ├─ phase_render()
  │    plugin.render(RenderCtx)    // GPU cmd recording + submit
  │
  └─ phase_present()
       renderer.present_image()    // vkQueuePresentKHR
       renderer.end_frame()        // advance frame counter
       tracy frame_mark()
```

窗口生命周期：

- 主线程收到 `WindowEvent::Resized` 后，只把最新尺寸写入 `AtomicU64`。
- 渲染线程在循环中读取最新尺寸；零尺寸表示最小化，跳过重建和渲染。
- 尺寸变化或 backend 报告 `need_resize` 时，统一调用 `FrameRuntime::recreate_swapchain_if_needed`。
- swapchain 重建完成后，runtime 调用 `AppPlugin::on_resize(ResizeCtx)`。

关闭生命周期：

- 主线程收到 `CloseRequested` 后设置 `shared.exit = true`，继续 pump event loop。
- 渲染线程观察到 exit 后销毁 runtime：先 `Gfx::wait_idle`，再 `plugin.shutdown`、`renderer.destroy`、`Gfx::destroy`。
- 渲染线程置位 `render_finished` 后，主线程退出 event loop、join 渲染线程，最后 drop `Window`。
- 这个顺序保证 Vulkan surface 和相关资源在 `Window` 销毁前已经释放。

## 3. 状态与所有权

状态和所有权回答四个问题：状态在哪里，谁拥有，谁能读写，什么时候同步。当前项目最关键的状态边界是 `World` 与 `RenderWorld`
的物理分离。

`World` 是 CPU 语义状态，表达“世界是什么”。它持有 `SceneManager` 和 `AssetHub`，面向 mesh、material、instance、light、asset
loading 等 CPU 侧概念。

`RenderWorld` 是 GPU 渲染状态和帧状态，表达“GPU 如何渲染它”。它持有 `GpuScene`、`BindlessManager`、`GlobalDescriptorSets`、
`GfxResourceManager`、`FifBuffers`、per-frame buffers、`FrameCounter`、`FrameSettings`、`PipelineSettings`、累积帧状态和时间值。

```text
Renderer
  -> World       CPU scene + assets
  -> RenderWorld GPU resources + frame state
```

`AppPlugin` 通过 typed contexts 获取阶段权限：

- `InitCtx`：可写 `World` 和 `RenderWorld`，用于一次性初始化。
- `UpdateCtx`：可写 `World` 和受控 settings，用于 CPU 更新，不暴露完整 `Renderer`。
- `RenderCtx`：只读 `RenderWorld`，附带 `RenderPresent`、GUI draw data 和 timeline，用于 GPU 命令录制。
- `ResizeCtx`：可写 `RenderWorld`，用于 swapchain resize 后重建 GPU 相关资源。

架构约束：

- 不把完整 `Renderer` 暴露给应用作为稳定 API。
- 不重新引入混合 CPU scene、GPU resource、调度策略和应用语义的大 `RenderContext`。
- update 阶段主要修改 CPU 语义状态，prepare 阶段同步到 GPU，render 阶段消费 prepare 后的稳定 GPU 输入。

## 4. 数据流向

数据流描述数据从哪里进入系统，在哪个边界发生语义转换，最终如何被 shader 和 present 消费。架构上应区分 CPU 语义数据、GPU
可见数据和 RenderGraph 中的临时资源。

数据流的总结：

```text
                  CPU side                              GPU side
  ┌──────────────────────────────┐     ┌──────────────────────────────┐
  │                              │     │                              │
  │  Disk ──▶ AssetLoader ──▶    │     │                              │
  │           AssetHub           │     │                              │
  │             │                │     │                              │
  │    gltf / obj / image        │     │                              │
  │             │                │     │                              │
  │             ▼                │     │                              │
  │       SceneManager           │upload│    GpuScene                  │
  │    (mesh/mat/instance)  ─────┼─────┼──▶ (structured buffer)       │
  │             │                │     │       │                      │
  │             │                │     │       ▼                      │
  │             │                │     │  Bindless handle table       │
  │             │                │     │  (tex / buffer / TLAS)       │
  │             │                │     │       │                      │
  │             │                │     │       ▼                      │
  │             │                │     │  GlobalDescriptorSets        │
  │             │                │     │  (perframe + bindless)       │
  │             │                │     │       │                      │
  │             │                │     │       ▼                      │
  │             │                │     │  RenderGraph                 │
  │             │                │     │  ┌──────────────────┐        │
  │  Plugin ────┼── build_graph ─┼─────┼─▶│ Pass A ──▶ B ──▶│ C      │
  │             │                │     │  │ (RT)  (accum) (blit)      │
  │             │                │     │  └──────────────────┘        │
  │             │                │     │       │                      │
  │             │                │     │  cmd record ──▶ submit       │
  │             │                │     │       │                      │
  │             │                │     │  ┌────▼──────┐               │
  │             │                │     │  │ Swapchain │               │
  │             │                │     │  │  Present  │               │
  │             │                │     │  └───────────┘               │
  └──────────────────────────────┘     └──────────────────────────────┘
```

RenderGraph 数据流：

- builder 导入外部资源，例如 swapchain image、FIF buffers 和已有 GPU resource。
- pass 在 `setup` 中声明 image/buffer 读写关系。
- compile 阶段做依赖分析、拓扑排序和 barrier 计算。
- execute 阶段按顺序录制 barrier 和 pass command。
- submit info 拼接 wait/signal semaphores，交给 queue submit。

架构约束：

- CPU scene 与 GPU scene 只通过 prepare/upload 边界衔接。
- pass 不直接修改 CPU scene。
- pass 必须声明资源读写关系，让 RenderGraph 推导同步，而不是依赖隐式执行顺序。

## 5. 调度与同步，线程模型

调度描述谁决定执行顺序；同步描述线程、GPU 命令和资源状态之间如何保持一致。本项目中调度分为 CPU phase 调度和 GPU pass
调度，线程模型分为 winit 主线程和渲染线程。

CPU 调度由 `FrameRuntime` 持有。外部只通过 public API 推进：`push_input_event`、`time_to_render`、
`recreate_swapchain_if_needed`、`run_frame`、`destroy`。内部阶段固定为
`begin_frame -> input -> update -> prepare -> render -> present`。prepare 阶段不暴露为 plugin hook，避免应用层分叉调度。

GPU 调度由 `RenderGraph` 表达。pass 声明资源读写后，RenderGraph 构建依赖图、做拓扑排序、计算 barrier，并在执行时录制命令。timeline
semaphore 控制 frames-in-flight 节奏，swapchain acquire/present 使用对应 binary semaphore。

线程模型：

```text
main thread
  owns EventLoop and Window
  sends InputEvent through channel
  writes latest size to AtomicU64
  sets exit flag

render thread
  owns FrameRuntime and Renderer
  creates, uses, destroys all Vulkan objects
  drains input events and runs frame loop
```

架构约束：

- 主线程不调用 Vulkan、`ash` 或 `truvis-gfx` API。
- 所有 Vulkan 对象在渲染线程创建、使用和销毁。
- resize 通过 latest-size 模式合并连续事件。
- GPU 同步优先通过 RenderGraph、semaphore 和 frame timeline 统一表达，避免上层散落手写同步。

## 6. 资源生命周期

资源应先按生命周期分类，再看具体 Vulkan 类型或 Rust 类型。架构审查时重点问：谁创建，谁持有，什么时候重建，什么时候销毁，是否跨帧或绑定窗口尺寸。

资源类别：

- Persistent：pipeline、sampler、descriptor layout、shader binding。通常跨多帧，随 renderer 或 pass 对象销毁。
- Frame：command buffer、per-frame buffer、FIF resources。受 `FrameCounter` 和 frames-in-flight 控制。
- Swapchain：swapchain image、image view、present semaphore、window-sized targets。绑定窗口尺寸，resize 后重建。
- Asset：texture、mesh buffer、material-related GPU resources。由 `AssetHub`、`GfxResourceManager`、`BindlessManager` 协作管理。
- RenderGraph transient：图内临时 image/buffer。生命周期绑定单次图构建与执行计划。

创建路径：

- `Renderer::new` 初始化 `Gfx`，创建 resource managers、`World` 和 `RenderWorld`。
- `Renderer::init_after_window` 创建 surface、swapchain 和 `RenderPresent`。
- `AppPlugin::init` 创建应用特定 pass、pipeline 或场景资源。

重建路径：

- resize 或 backend `need_resize` 统一进入 `FrameRuntime::recreate_swapchain_if_needed`。
- runtime 驱动 `renderer.recreate_swapchain` 和 `renderer.resize_frame_buffer`。
- 重建完成后调用 `plugin.on_resize`。

销毁路径：

- `FrameRuntime::destroy` 先等待 GPU idle，再执行 `plugin.shutdown`、`renderer.destroy`、`Gfx::destroy`。
- Vulkan 资源必须在渲染线程销毁。
- 主线程必须等渲染线程完成资源销毁后再 drop `Window`。
