# Render View 概念引入分析（2026-04-23）

> 本文讨论 Truvis 是否需要抽象出 `View` 概念，以及 `View` 在当前
> `World + RenderWorld + FrameRuntime + RenderGraph` 架构中应该承担什么职责。
>
> 对照对象：
>
> - Falcor：RenderGraph / Scene / Camera / RenderPass 组合中的 view-like 输入
> - Filament：`View` 作为 Scene、Camera、Viewport、RenderTarget、渲染选项的组合入口
> - Bevy：Extracted View / Render World 中的 camera-driven view entity
> - Unreal Engine：`FSceneView` / `FSceneViewFamily` 作为较重的视图与视图族描述

---

## 1. 总体判断

Truvis 概念上应该有 `View`，但不建议一开始做成 UE 那种重型
`SceneView / ViewFamily` 系统。

更适合当前项目的方向是：

```text
View = 一次渲染视角 / 输出意图的描述
```

也就是把现在散落在以下对象中的“主视角渲染”语义显式化：

- `Camera`：提供 view/projection/camera position。
- `FrameSettings`：提供当前 frame extent / format。
- `AccumData`：追踪当前 camera 的累积帧状态。
- `PerFrameData`：shader 可见的 view/projection/camera/resolution/accum 数据。
- `FifBuffers`：默认承载 main view 的 single-frame、gbuffer、accum、render-target 图像。
- `RtPipeline`：默认按 main view 构建 ray tracing / denoise / tone mapping / resolve 流程。

当前代码里实际已经存在一个隐式的 `main view`，只是它没有被命名。
`FrameRuntime` 持有一个 `CameraController`，每帧把 camera 交给
`Renderer::update_accum_frames()` 和 `Renderer::before_render()`，
再由 demo pipeline 从 `RenderWorld.frame_settings` 和 `RenderWorld.fif_buffers`
中推导渲染目标。

因此建议：

```text
短期：引入轻量 MainView，让现有隐式主视角显式化。
中期：把 per-frame 中的 camera 数据重命名/迁移为 per-view 数据。
长期：支持多个 View，例如 editor viewport、shadow view、reflection view、probe view。
```

---

## 2. View 应该代表什么

`View` 不等于 camera。

Camera 只回答“从哪里看、怎么投影”。View 还需要回答：

```text
1. 看哪个 scene / scene subset？
2. 使用哪个 camera-derived matrix？
3. 画到多大的 viewport / extent？
4. 输出到哪个 render target？
5. 本 view 的渲染选项是什么？
6. 本 view 的 temporal state 是什么？
```

对当前 Truvis，第一版可以把 scene subset 先固定为全 scene，不急着做 visibility
mask / layer / culling set。

一个合适的 CPU 侧描述可以是：

```rust
pub struct ViewDesc {
    pub id: ViewId,
    pub name: String,

    pub camera: CameraSnapshot,
    pub viewport: Viewport,
    pub extent: vk::Extent2D,

    pub target: ViewTarget,
    pub settings: ViewSettings,
}
```

`ViewDesc` 表达应用和 runtime 的渲染意图，不直接持有 GPU command、descriptor
或 RenderGraph 资源。

渲染准备阶段可以生成：

```rust
pub struct PreparedView {
    pub id: ViewId,

    pub view: glam::Mat4,
    pub projection: glam::Mat4,
    pub inv_view: glam::Mat4,
    pub inv_projection: glam::Mat4,

    pub camera_pos: glam::Vec3,
    pub camera_forward: glam::Vec3,

    pub viewport: Viewport,
    pub extent: vk::Extent2D,

    pub per_view_buffer: GfxStructuredBuffer<gpu::PerFrameData>,
    pub accum_state: AccumData,
}
```

这里的命名重点是区分：

```text
ViewDesc      CPU 侧意图
PreparedView  render prepare 后的 GPU 可消费数据
```

---

## 3. View 应该包含哪些内容

### 3.1 第一阶段最小字段

第一阶段只服务现有 main view，可以包含：

- `ViewId`
- `CameraSnapshot`
- `Viewport`
- `vk::Extent2D`
- `ViewTarget::MainFif`
- `ViewSettings`
- `PerViewData` / 现有 `gpu::PerFrameData`
- per-view `AccumData`

`CameraSnapshot` 建议从当前 `Camera` 中提取稳定数据：

```rust
pub struct CameraSnapshot {
    pub position: glam::Vec3,
    pub forward: glam::Vec3,
    pub view: glam::Mat4,
    pub projection: glam::Mat4,
    pub inv_view: glam::Mat4,
    pub inv_projection: glam::Mat4,
    pub fov_deg_vertical: f32,
    pub near: f32,
}
```

这样 pass 不需要知道 `Camera` 的 Euler 表达方式，也不会依赖
`truvis-renderer::platform::camera::Camera` 这种平台/runtime 侧类型。

### 3.2 后续可以扩展的字段

后续需要多视角时，可以继续加入：

- visibility mask / layer mask
- culling result handle
- jitter / TAA sample index
- exposure / tone mapping override
- LOD bias
- debug view mode
- per-view render feature flags
- view history resources
- scene subset / render layer

这些不要第一阶段就塞进去，否则 `View` 会太早变成新的大袋子。

---

## 4. View 不应该包含什么

`View` 不应该拥有下面这些对象：

- `World`
- `SceneManager`
- `AssetHub`
- `BindlessManager`
- `GfxResourceManager`
- swapchain acquire/present semaphore
- command buffer
- `RenderGraphBuilder`
- 具体 pass 实例，例如 RT pass、denoise pass、resolve pass

原因是这些对象分别属于已有边界：

| 对象 | 所属边界 |
|---|---|
| `World` / `SceneManager` / `AssetHub` | CPU domain state |
| `RenderWorld` / managers / FIF resources | GPU render state |
| swapchain / semaphore / present image | present/surface |
| command buffer / submit | backend execution |
| RenderGraph | pass 编排 |
| concrete passes | render-passes / pipeline |

`View` 应该只是把“这一次从什么视角渲染到什么目标”说清楚，而不是变成新的全局上下文。

---

## 5. View 应该放在哪个层级

建议把基础 `View` 类型放在 `truvis-render-interface`，或者未来重命名后的
`truvis-render-core`。

理由：

```text
1. View 是 renderer、runtime、passes 都需要理解的渲染契约。
2. View 不应该依赖 scene / asset / app。
3. RenderGraph 应继续只负责资源依赖和 pass 编排，不应该知道 camera 语义。
4. app-api 可以通过 View 的窄接口避免暴露 renderer concrete type。
```

推荐位置：

```text
engine/crates/truvis-render-interface/src/view.rs
```

初始类型：

```text
ViewId
Viewport
CameraSnapshot
ViewDesc
PreparedView
ViewSettings
ViewTarget
```

当前分层里的使用关系可以是：

```text
truvis-frame-runtime
  CameraController -> main ViewDesc

truvis-renderer
  ViewDesc -> PreparedView
  upload per-view uniform
  update per-view accum

truvis-render-passes
  read PreparedView / PerViewData

truvis-app
  pipeline receives main view and builds RenderGraph
```

不建议把 `View` 放进 `truvis-render-graph`。RenderGraph 仍应保持
scene/asset/app 无关，只看资源 handle、pass 声明和同步状态。

---

## 6. 引入 View 后的渲染流程

当前简化流程：

```text
begin_frame
  -> input
  -> build_ui
  -> update
  -> camera_controller.update
  -> renderer.update_accum_frames(camera)
  -> renderer.before_render(camera)
  -> plugin.render(RenderCtx { render_world, render_present, ... })
  -> pipeline 从 RenderWorld.frame_settings / fif_buffers 构建 graph
  -> present
end_frame
```

引入 `View` 后建议演进为：

```text
begin_frame
  -> input
  -> build_ui
  -> update

  -> build_views
       main_view_desc = camera + swapchain extent + main target + settings

  -> extract / prepare
       update assets
       extract scene render data
       upload GPU scene

       prepare main view
         - compute view/projection/inverse matrices
         - update per-view accum
         - upload per-view uniform buffer
         - update descriptors

  -> render
       plugin.render(RenderCtx {
         render_world,
         views,
         render_present,
         gui_draw_data,
         timeline,
       })

       RtPipeline::render(main_view)
         - ray-tracing(view)
         - denoise-accum(view)
         - blit / sdr(view)
         - resolve view target to swapchain
         - gui

  -> present
end_frame
```

这样 `Renderer::before_render(&Camera)` 可以逐步演进成：

```rust
renderer.prepare_frame(&world);
let main_view = renderer.prepare_view(main_view_desc);
```

`RenderCtx` 也可以从只暴露 `render_world`，变成显式暴露 main view 或 view list：

```rust
pub struct RenderCtx<'a> {
    pub render_world: &'a RenderWorld,
    pub main_view: &'a PreparedView,
    pub render_present: &'a RenderPresent,
    pub gui_draw_data: &'a imgui::DrawData,
    pub timeline: &'a GfxSemaphore,
}
```

更长期可以改为：

```rust
pub struct RenderCtx<'a> {
    pub render_world: &'a RenderWorld,
    pub views: &'a ViewStore,
    pub surfaces: &'a SurfaceStore,
    pub gui_draw_data: &'a imgui::DrawData,
    pub timeline: &'a GfxSemaphore,
}
```

---

## 7. 与现有类型的关系

### 7.1 Camera

`Camera` 继续保留为 runtime/input 层的可变控制对象。

`View` 不直接持有 `Camera`，而是持有 `CameraSnapshot` 或 matrix-ready data。
这样可以避免 render-interface 依赖 `truvis-renderer::platform::camera::Camera`。

长期看，`Camera` 本身也可以从 `truvis-renderer` 下沉到更合适的 crate，
但这不是引入 View 的前置条件。

### 7.2 PerFrameData

当前 shader 的 `PerFrameData` 实际混合了两类数据：

```text
per-frame:
  time
  delta time
  frame id

per-view:
  projection
  view
  inv_view
  inv_projection
  camera_pos
  camera_forward
  resolution
  accum_frames
```

第一阶段可以继续沿用 `gpu::PerFrameData`，但在 Rust 侧把它作为
per-view uniform 使用。

中期可以拆成：

```text
FrameData
  time
  delta_time
  frame_id

ViewData
  matrices
  camera
  viewport/resolution
  accum
```

为了减少 shader 和 binding 震荡，不建议和 `View` 第一版一起完成拆分。

### 7.3 AccumData

当前 `AccumData` 在 `RenderWorld` 中是全局状态，这等价于只支持一个 main view。

引入 `View` 后，`AccumData` 应该逐步变成 per-view state：

```text
RenderWorld
  view_states[ViewId].accum_data
```

对于主视角，第一阶段可以仍然复用现有 `RenderWorld.accum_data`，但 API 命名上应该
朝 `update_view_accum(view_id, camera_snapshot)` 靠拢。

### 7.4 FifBuffers

当前 `FifBuffers` 是默认 main view 的图像集合：

```text
single_frame_rt
gbuffer_a/b/c
accum_image
render_target
```

第一阶段可以让 `ViewTarget::MainFif` 显式引用这套资源。

长期如果支持多个 offscreen view，可以演进成：

```text
ViewTarget::MainFif
ViewTarget::ImportedImage(...)
ViewTarget::OwnedHistory(...)
ViewTarget::Swapchain(...)
```

不要让 `View` 自己创建或销毁这些图像；它只引用目标意图或目标句柄。

---

## 8. 为什么这个抽象值得做

### 8.1 多 viewport / editor 支持

没有 `View` 时，全局只有一个 camera 和一个 frame extent。
一旦需要 editor viewport、game viewport、preview viewport，就会出现很多特殊参数。

有 `View` 后：

```text
main_view
editor_scene_view
material_preview_view
```

都可以走同一套 prepare 和 render contract。

### 8.2 Shadow / reflection / probe

Shadow map、reflection probe、irradiance probe 本质上也是从某个位置/方向渲染到某个目标。

如果没有 View，这些功能往往会绕过主渲染流程，形成独立的 ad-hoc path。

### 8.3 Temporal state 归属更准确

TAA、progressive RT accumulation、history buffer 都是 per-view 的。

当前 `AccumData` 是全局的，只适用于单 main view。引入 View 可以避免未来多视角时
history 互相污染。

### 8.4 Pass 输入更清晰

当前 pass 经常通过 `RenderWorld.frame_settings.frame_extent` 推断当前渲染尺寸。

有 View 后，pass 可以明确接收：

```text
view.extent
view.viewport
view.per_view_buffer
view.target
```

这比从全局状态里猜“当前画的是哪个视角”更稳定。

---

## 9. 与开源渲染器概念对照

### 9.1 Filament

Filament 的 `View` 是很好的参考对象：它把 scene、camera、viewport、render target、
渲染选项组织为 renderer 的输入。

Truvis 可以借鉴这种轻量边界：

```text
Renderer.render(view)
```

但不必照搬其完整 API。Truvis 当前仍有自定义 `FrameRuntime`、`RenderGraph` 和
pass pipeline，`View` 应该服务现有编排方式。

### 9.2 Bevy

Bevy 的启发是 extracted view。

Main World 中 camera/entity 的状态不会直接被 render pass 使用，而是 extract 到
Render World，形成渲染侧稳定数据。

Truvis 可以借鉴：

```text
CameraController / app state
  -> ViewDesc
  -> PreparedView
  -> render pass
```

不需要引入 ECS，也不需要完整 Bevy schedule。

### 9.3 Unreal Engine

UE 的 `FSceneView` / `FSceneViewFamily` 很强大，但也很重。

它包含大量 editor、show flag、stereo、family、visibility、postprocess、view state
等信息。Truvis 现在不适合一上来做这种完整模型。

可以先保留一个未来扩展点：

```text
View
ViewGroup / ViewFamily
```

但第一阶段只实现单 view，不做 view family。

### 9.4 Falcor

Falcor 更像研究型 renderer，RenderGraph 和 RenderPass 经常围绕 camera、scene、
render target 组合。

Truvis 可借鉴的是：render pass 不应到处寻找全局主相机，而应该从 pipeline 或
graph 编排处拿到明确的 view input。

---

## 10. 推荐演进步骤

### P0：只命名 main view，不改变能力

新增基础类型：

```text
truvis-render-interface/src/view.rs
  ViewId
  Viewport
  CameraSnapshot
  ViewDesc
  PreparedView
  ViewTarget
  ViewSettings
```

当前只创建一个：

```text
ViewId::MAIN
```

### P0：把 camera 到 GPU uniform 的转换收束到 prepare_view

当前：

```rust
renderer.update_accum_frames(camera);
renderer.before_render(camera);
```

建议演进为：

```rust
let main_view_desc = runtime.build_main_view_desc();
let main_view = renderer.prepare_view(main_view_desc);
renderer.before_render(&main_view);
```

第一阶段可以内部仍然调用现有逻辑，只是 API 开始表达 view。

### P1：让 RtPipeline 接收 PreparedView

当前 `RtPipeline::render()` 主要依赖：

```text
render_world.frame_settings.frame_extent
render_world.fif_buffers
render_world.per_frame_data_buffers
render_world.accum_data
```

建议逐步改成：

```rust
RtPipeline::render(
    &self,
    render_world: &RenderWorld,
    main_view: &PreparedView,
    render_present: &RenderPresent,
    gui_draw_data: &imgui::DrawData,
    frame_fence: &GfxSemaphore,
)
```

pass data 使用 `main_view.extent` 和 `main_view.per_view_buffer`，而不是从全局 frame
settings 推断。

### P1：`PerFrameData` 语义改为 per-view 使用

先不急着改 shader 文件名和 binding 类型。

可以先在 Rust 侧注释/文档中明确：

```text
当前 gpu::PerFrameData 实际作为 main view uniform 上传。
后续会拆分 FrameData / ViewData。
```

### P2：ViewStore

当需要多 view 时，再引入：

```rust
pub struct ViewStore {
    pub main: ViewId,
    views: Vec<PreparedView>,
}
```

并支持：

```text
get(main)
iter()
get_by_name()
```

### P2：ViewTarget 抽象 render target

第一阶段可以只有：

```rust
pub enum ViewTarget {
    MainFif,
}
```

后续再扩展：

```rust
pub enum ViewTarget {
    MainFif,
    Offscreen(ViewTargetHandle),
    Imported {
        image: GfxImageHandle,
        view: GfxImageViewHandle,
        format: vk::Format,
        extent: vk::Extent2D,
    },
}
```

### P3：ViewGroup / ViewFamily

只有当出现 stereo、split-screen、multi-viewport editor 或 shared post-process chain 时，
再引入：

```text
ViewGroup
ViewFamily
```

不要提前做。

---

## 11. 风险与边界

### 11.1 不要把 View 做成新的 RenderContext

如果 `View` 开始持有 manager、command buffer、scene、asset、graph builder，
它就会重复旧 `RenderContext` 的问题。

判定标准：

```text
View 能不能被 Clone / Snapshot / Debug 打印？
View 是否需要 destroy GPU resource？
View 是否能脱离具体 pass 存在？
```

如果答案开始偏向“不能”，说明边界变重了。

### 11.2 不要为了 View 立即大改 shader

`PerFrameData` 的命名确实不准，但 shader binding 改名会牵动较多文件。

建议先在 Rust 侧引入 View，再择机拆分：

```text
FrameData
ViewData
```

### 11.3 不要让 RenderGraph 依赖 View

RenderGraph 可以接收 pass 闭包捕获的 `PreparedView` 引用，但 graph crate 本身不应该
知道 `View` 语义。

也就是说：

```text
truvis-app / pipeline 知道 View
truvis-render-passes 可以知道 View
truvis-render-graph 不知道 View
```

这能保持图编排层的纯度。

---

## 12. 建议的目标形态

一个比较健康的中期形态：

```text
FrameRuntime
  -> input/update camera
  -> build main ViewDesc
  -> renderer.prepare_frame()
  -> renderer.prepare_view(main_view_desc)
  -> plugin.render(RenderCtx { main_view, render_world, ... })

Renderer
  -> owns World + RenderWorld
  -> extracts scene
  -> uploads GPU scene
  -> prepares views
  -> updates descriptors

RenderWorld
  -> owns GPU resources/managers
  -> owns view states / per-view buffers
  -> owns FIF buffers for main view

RtPipeline
  -> receives PreparedView
  -> builds graph for that view
```

对应的概念边界：

```text
Camera       可交互的相机状态
ViewDesc     渲染意图
PreparedView 渲染侧已准备好的视图数据
RenderWorld  GPU 状态和资源所有权
RenderGraph  pass/resource dependency DAG
Pipeline     如何为某个 View 组织 passes
```

---

## 13. 结论

Truvis 应该引入 `View`，但应该以轻量、数据导向、单 main view 起步。

第一版 `View` 的价值不是马上支持多视角，而是把当前隐式主视角从
`Camera + FrameSettings + AccumData + PerFrameData + FifBuffers + RtPipeline`
中明确拆出来。

推荐路线：

```text
1. 在 render-interface/render-core 中定义轻量 View 类型。
2. FrameRuntime 每帧构建 main ViewDesc。
3. Renderer 把 ViewDesc prepare 成 PreparedView。
4. RtPipeline 和 pass 接收 PreparedView。
5. 暂时保留现有 PerFrameData 和 MainFif，避免一次性大改。
6. 等 editor/shadow/probe 等需求出现，再扩展 ViewStore / ViewTarget / ViewGroup。
```

这条路线能保持现有分层不被打穿，同时为后续多 viewport、shadow、reflection、
probe、TAA/history 等能力留下清晰入口。
