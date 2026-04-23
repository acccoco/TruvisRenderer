# 当前项目结构职责诊断与开源渲染器对照（2026-04-23）

> 本文基于 2026-04-23 的代码现状，对当前模块职责、依赖边界和命名进行一次结构诊断。
> 重点不是重新描述目录结构，而是回答一个问题：为什么现在看起来“类型职责有些混乱”。
>
> 对照对象：
> - Bevy：Main World / Render World / Extract / Prepare / Queue / Render
> - Filament：Engine / Renderer / SwapChain / View / Scene / Camera
> - Godot：Scene / Server / Driver/Platform 分层
> - bgfx：低层跨 API 渲染库，强调 Bring Your Own Engine/Framework
> - Falcor：Core framework / RenderPasses / Samples / Tools / RenderGraph

---

## 1. 总体判断

当前结构不是完全混乱，而是处在多轮重构后的中间态。

已经完成并且方向正确的部分：

- `truvis-winit-app` 已经从渲染状态中剥离，主要负责 winit 事件循环和渲染线程入口。
- `truvis-frame-runtime` 明确成为帧编排器，外部通过 `push_input_event` / `time_to_render` / `run_frame` / `destroy` 驱动。
- `truvis-app-api` 把 `AppPlugin` 和 typed contexts 从 demo 应用中拆了出来。
- `truvis-renderer::Renderer` 当前已经持有 `World + RenderWorld`，不再是旧文档里描述的单一 `RenderContext` 大袋子。
- `truvis-render-graph` 已经不再依赖 scene/asset，图编排层的方向是干净的。
- `truvis-gui-backend` 基本保持为 Vulkan ImGui 后端，`GuiRgPass` 放在上层适配。

真正造成职责混乱感的，是以下几个边界对象仍然知道太多：

- `truvis-scene` 知道 asset、bindless、shader binding。
- `truvis-asset` 知道 bindless SRV 注册。
- `truvis-render-interface` 名为 interface，但实际包含大量具体 manager、`RenderWorld`、`GpuScene`、`RenderData`。
- `truvis-app-api` 作为契约层，却直接暴露 `truvis-renderer` 中的具体 `Camera` / `RenderPresent` 类型。
- `truvis-app` 同时承载 demo、RenderGraph glue、pipeline 组装和过渡期依赖。
- `RenderPresent` 同时持有 swapchain present 资源和 `GuiBackend`。

一句话：目录已经分层，但“谁允许知道谁”还没有完全收紧。

---

## 2. 当前结构速览

当前主线可以理解为：

```text
truvis-winit-app
  -> truvis-frame-runtime::FrameRuntime
    -> truvis-renderer::Renderer
      -> World + RenderWorld
        -> scene / asset / render-interface / render-graph / gfx
```

核心 crate 的职责大致如下：

| Crate | 当前定位 | 主要问题 |
|---|---|---|
| `truvis-gfx` | Vulkan RHI | 基本清晰 |
| `truvis-render-interface` | 渲染核心资源与状态 | 名字像接口，内容是 concrete core；还混有 `GpuScene` / `RenderData` |
| `truvis-render-graph` | RenderGraph 编排 | 当前相对健康 |
| `truvis-scene` | CPU 场景 | 依赖 asset / bindless / shader，越过 CPU 场景边界 |
| `truvis-asset` | 异步资产加载 | 直接注册 bindless SRV，耦合 GPU 可见性策略 |
| `truvis-world` | CPU World 聚合 | 目前很薄，仅聚合 `SceneManager + AssetHub` |
| `truvis-renderer` | GPU backend + state owner | 仍承担 extract/prepare/asset 更新等调度性行为 |
| `truvis-app-api` | 插件契约 | 契约层泄漏 renderer concrete type |
| `truvis-frame-runtime` | 帧编排 | Camera/Input/GUI/Overlay 硬编码，不是 plugin 化 |
| `truvis-render-passes` | 通用 pass | 依赖 `truvis-world`，说明 pass 层仍可触达 CPU World |
| `truvis-app` | demo + pipeline glue | 依赖扇出大，职责混合 |
| `truvis-winit-app` | 平台入口 | 当前方向正确 |

---

## 3. 关键职责混乱点

### 3.1 `truvis-scene` 不够 CPU-only

当前 `SceneManager` 直接依赖：

```rust
use truvis_asset::asset_hub::AssetHub;
use truvis_render_interface::bindless_manager::{BindlessManager, BindlessSrvHandle};
use truvis_render_interface::render_data::{InstanceRenderData, MaterialRenderData, MeshRenderData, RenderData};
use truvis_shader_binding::gpu;
```

并且 `prepare_render_data()` 会从 `AssetHub` 查纹理，再向 `BindlessManager` 查询 shader 可见的 SRV handle。

这说明 scene 层同时在回答两个问题：

```text
1. 场景里有什么？                 <- scene 应该回答
2. shader 应该如何访问这些资源？   <- render/extract 层应该回答
```

建议方向：

```text
SceneManager::snapshot()
  -> SceneSnapshot（纯 CPU 语义：mesh/material/instance/light/path/handle）

SceneBridge / Extract
  -> 读取 SceneSnapshot + AssetHub + BindlessManager
  -> 生成 RenderData（GPU 上传中间表示）

GpuScene
  -> upload_render_data()
```

收益：

- `scene` 不再依赖 `asset`、`BindlessManager`、`RenderData`。
- CPU 场景可以更容易做 headless 测试。
- CPU 语义和 GPU 可见性策略分离。

### 3.2 `truvis-asset` 直接碰 bindless

当前 `AssetHub::new()` 和 `AssetHub::update()` 接收 `BindlessManager`，并在 fallback texture 和 texture ready 时调用 `register_srv()`。

资产层的理想职责应是：

```text
路径 -> 加载状态 -> CPU decode -> GPU image/image view ready
```

bindless 注册则属于：

```text
GPU image view ready -> shader visible descriptor index
```

后者更适合放在 renderer/backend 的 prepare 阶段，或独立的 asset-extract/asset-prepare system 中。

建议方向：

```rust
pub struct AssetReadyEvent {
    pub texture: AssetTextureHandle,
    pub view_handle: GfxImageViewHandle,
}

impl AssetHub {
    pub fn update(&mut self, resource_manager: &mut GfxResourceManager) -> Vec<AssetReadyEvent>;
}

impl Renderer {
    fn register_ready_assets(&mut self, events: Vec<AssetReadyEvent>) {
        for event in events {
            self.render_world.bindless_manager.register_srv(event.view_handle);
        }
    }
}
```

### 3.3 `truvis-render-interface` 名不副实

`truvis-render-interface` 当前包含：

```text
bindless_manager
cmd_allocator
frame_counter
geometry
gfx_resource_manager
global_descriptor_sets
gpu_scene
handles
pipeline_settings
render_data
render_world
sampler_manager
fif_buffer
stage_buffer_manager
```

这不是一个薄 interface crate，而是渲染核心状态和资源管理层。

更合适的命名候选：

| 名称 | 含义 | 评价 |
|---|---|---|
| `truvis-render-core` | 渲染核心基础设施 | 最推荐，短且准确 |
| `truvis-render-state` | 渲染状态管理 | 突出 `RenderWorld`，但低估 manager 行为 |
| `truvis-render-infra` | 渲染基础设施 | 准确但稍显工程化 |

进一步清理方向：

- `Handles / FrameCounter / GfxResourceManager / CmdAllocator / BindlessManager / GlobalDescriptorSets` 留在 `render-core`。
- `GpuScene / RenderData` 可考虑上移到 `truvis-renderer`，或拆到 `truvis-render-scene`。
- `RenderWorld` 可以保留，但它应只表达 GPU state，不应成为跨层万能袋子。

### 3.4 `truvis-app-api` 泄漏具体 renderer 类型

当前 app-api 的 `InitCtx` / `RenderCtx` / `ResizeCtx` 直接暴露：

```rust
truvis_renderer::platform::camera::Camera
truvis_renderer::present::render_present::RenderPresent
```

这会让“插件契约”依赖具体 renderer 实现。

理想上，app-api 应该暴露窄视图：

```text
CameraView / CameraMut
SurfaceInfo / SwapchainInfo
RenderTargetInfo
RenderWorldView
CommandContext
```

不应该直接把 `RenderPresent` 这种 backend-owned object 交给应用层。

### 3.5 `FrameRuntime` 仍是 builtin systems 合订本

`FrameRuntime` 当前硬编码持有：

```text
Renderer
CameraController
InputManager
GuiHost
Box<dyn AppPlugin>
Vec<Box<dyn OverlayModule>>
```

这比旧的 `RenderApp::big_update()` 清楚很多，但还不是插件化架构。

推荐渐进路线：

```text
当前：FrameRuntime 直接持有 Camera/Input/GUI
  -> 小步：为这些组件抽 trait，允许替换实现
  -> 中步：BuiltinPlugin（InputPlugin / CameraPlugin / ImGuiPlugin / OverlayPlugin）
  -> 大步：固定 Phase + 有序 Plugin 列表
```

不建议一上来实现完整 Bevy Schedule。对于渲染器，固定 phase 足够：

```text
init -> input -> update -> extract -> prepare -> render -> present -> shutdown
```

### 3.6 `RenderPresent` 混入 GUI backend

`RenderPresent` 当前同时包含：

```text
GfxSurface
GfxSwapchain
swapchain image handles/views
present/render semaphores
GuiBackend
```

swapchain 和 GUI 后端没有强绑定关系。GUI 可以画到 swapchain，也可以画到 offscreen render target。

建议方向：

```text
Surface / Swapchain / PresentSemaphores -> SurfaceRegistry 或 RenderSurface
GuiBackend / GuiMesh / ImGui texture map -> ImGuiRenderResources 或 GuiRenderFeature
GuiRgPass -> renderer/backend 或 render-feature 层
```

---

## 4. 与开源渲染器项目对照

### 4.1 Bevy

Bevy 的渲染架构核心启发是：

```text
Main World
  -> Extract
Render World
  -> Prepare
  -> Queue
  -> Render
```

对 Truvis 的对应关系：

| Bevy 概念 | Truvis 当前对应 | 差距 |
|---|---|---|
| Main World | `truvis-world::World` | 目前太薄，且 scene/asset 内部仍碰 GPU 策略 |
| Render World | `RenderWorld` | 已有，但还和 backend owner 紧密绑定 |
| Extract | `SceneManager::prepare_render_data()` + `GpuScene::upload_render_data()` 前半段 | 没有显式 phase，藏在 `Renderer::before_render()` |
| Prepare | `Renderer::before_render()` / descriptor update / asset update | prepare 与 extract 混在一起 |
| Render | `AppPlugin::render()` 构建并执行 RenderGraph | 基本方向正确 |

可借鉴但不必照搬：

- 借鉴 Main/Render 双世界与 Extract 边界。
- 不必引入完整 ECS。
- 不必实现复杂 Schedule，固定 phase + 有序 plugin 就够。

### 4.2 Filament

Filament 的公开 API 中，常见对象边界是：

```text
Engine
Renderer
SwapChain
View
Scene
Camera
```

它的启发是：窗口输出、渲染执行、场景内容、观察视图是不同对象。

对 Truvis 的建议：

- `RenderPresent` 不应被 app-api 直接暴露。
- `RenderCtx` 更适合暴露 `View/Surface/RenderTarget` 类的窄接口。
- `SceneManager` 应只表达 scene 内容，不负责 shader-visible descriptor 解析。

### 4.3 Godot

Godot 架构强调：

```text
Scene Layer
Server Layer
Drivers / Platform Interface
```

RenderingServer / RenderingDevice 通过 RID 或 backend abstraction 把 scene 和 driver 隔开。

对 Truvis 的建议：

- CPU scene 不应 include GPU descriptor 细节。
- GPU resource handle 应尽量 opaque。
- asset 和 scene 可以产出“语义句柄”，渲染层再翻译成 GPU resource / descriptor。

### 4.4 bgfx

bgfx 的定位是跨 API 低层渲染库，强调 Bring Your Own Engine/Framework。

对 Truvis 的建议：

- `truvis-gfx` 应继续保持像 bgfx 一样偏低层、偏 platform/backend agnostic。
- winit 只负责 native handle、事件泵和线程入口，不应进入 gfx/core 层。
- 上层 runtime 不应把 `Gfx::get()` 式全局访问扩散到业务层。

### 4.5 Falcor

Falcor 适合参考研究型 renderer 的组织方式：

```text
Core framework
RenderPasses
Samples
Tools
RenderGraph / RenderGraphEditor
```

对 Truvis 的建议：

- `truvis-render-passes` 的方向是对的，但不应依赖 CPU `World`。
- `truvis-app` 里 demo、pipeline 组装、RenderGraph glue 可以进一步拆开。
- 示例应用最好不要承担 framework crate 的职责。

---

## 5. 建议目标结构

不建议一步到位做成完整游戏引擎。更适合 Truvis 的目标结构是：

```text
L0 Foundation
  truvis-utils / truvis-logs / truvis-path
  shader-binding / descriptor-layout

L1 RHI
  truvis-gfx

L2 Render Core
  handles / frame counter / cmd allocator
  gfx resource manager / bindless / descriptor sets
  frame settings / pipeline settings

L3 Domain Modules
  truvis-scene        CPU 场景语义
  truvis-asset        资产加载与状态
  truvis-render-graph pass DAG 与资源依赖
  truvis-gui-backend  ImGui Vulkan 后端

L4 Renderer Backend
  RenderBackend / Renderer
  World + RenderWorld owner
  SceneBridge / AssetBridge / GpuScene prepare
  surface/swapchain/present
  submit/sync

L5 Runtime + App API
  FrameRuntime
  AppPlugin / Plugin list
  typed contexts
  built-in input/camera/gui/overlay plugins

L6 Platform + Examples
  truvis-winit-app
  triangle / rt-cornell / rt-sponza / shader-toy
```

---

## 6. 推荐改造顺序

### P0：拆 `SceneManager::prepare_render_data()`

目标：

```text
SceneManager.prepare_render_data(bindless, asset_hub)
  -> SceneManager.snapshot()
  -> SceneBridge.translate(snapshot, asset_hub, bindless)
```

收益最大，能立刻让 `scene` 层变清爽。

影响：

- `truvis-scene`
- `truvis-renderer`
- `truvis-render-interface` 或未来 `truvis-render-core`

### P0：让 `AssetHub::update()` 不再注册 bindless

目标：

```text
AssetHub::update(resource_manager) -> Vec<AssetReadyEvent>
Renderer::register_ready_assets(events, bindless)
```

收益：

- asset 层不再知道 descriptor heap。
- bindless 注册策略集中在 render/backend。

### P1：显式 `phase_extract`

当前：

```text
phase_prepare
  -> update_accum_frames
  -> before_render
       -> update_gpu_scene
       -> update_perframe_descriptor_set
```

建议：

```text
phase_extract
  -> scene snapshot
  -> asset ready events
  -> build extracted render data

phase_prepare
  -> upload extracted data
  -> update descriptors
  -> update per-frame buffers
```

### P1：`truvis-render-interface` 改名或收窄

最小路线：

```text
truvis-render-interface -> truvis-render-core
```

更彻底路线：

```text
GpuScene / RenderData -> renderer 或 render-scene
RenderWorld 保留在 render-core
```

### P2：收窄 app-api contexts

目标：

```text
RenderCtx 不直接暴露 RenderPresent
InitCtx 不直接暴露 backend-owned object
ResizeCtx 只暴露 surface/swapchain/framebuffer 信息视图
```

### P2：拆 `truvis-app`

建议：

```text
truvis-app              plugin glue / public app helpers
truvis-render-pipelines RT pipeline / shadertoy pipeline / raster pipeline
truvis-examples         demos
```

### P3：多 plugin 与 builtin plugin

目标：

```rust
pub trait Plugin {
    fn init(&mut self, _ctx: &mut InitCtx) {}
    fn input(&mut self, _ctx: &mut InputCtx) {}
    fn update(&mut self, _ctx: &mut UpdateCtx) {}
    fn extract(&mut self, _ctx: &mut ExtractCtx) {}
    fn prepare(&mut self, _ctx: &mut PrepareCtx) {}
    fn render(&mut self, _ctx: &RenderCtx) {}
    fn present(&mut self, _ctx: &mut PresentCtx) {}
    fn shutdown(&mut self) {}
}
```

注意：这里不需要完整 ECS 或复杂 schedule。固定 phase + 插件顺序声明即可。

---

## 7. 架构健康度检查清单

后续每次新增 crate / module / pass 时，可以用这组问题自查：

```text
1. scene 层是否依赖 GPU descriptor / bindless / command buffer？
2. asset 层是否直接注册 shader-visible descriptor？
3. render-graph 是否知道 scene / asset / app？
4. app-api 是否暴露 renderer 的 concrete private object？
5. pass 是否能访问整个 World，而不是只访问必要的 RenderWorld view？
6. GUI backend 是否被 swapchain/present 强绑定？
7. app crate 是否同时承担 demo、pipeline、framework、adapter 多个角色？
8. Renderer 的方法是在“执行 backend 操作”，还是在“决定调度顺序”？
9. 是否存在可单独命名的 Extract / Prepare 边界？
10. 文档中的结构是否仍然匹配代码现状？
```

---

## 8. 结论

当前 Truvis 的方向是对的：它已经从单体应用循环走向了 `FrameRuntime + AppPlugin + World + RenderWorld + RenderGraph` 的结构。

下一步不要先大改目录，而要先收紧依赖方向：

```text
scene 不知道 bindless
asset 不知道 bindless
app-api 不知道 RenderPresent
render-passes 不知道 World
renderer/backend 负责 CPU -> GPU 的桥接
FrameRuntime 显式拥有 extract / prepare phase
```

只要先完成 `SceneBridge` 和 `AssetReadyEvent` 这两个小而关键的边界，项目的职责清晰度会明显提升。

---

## 9. 参考资料

- Bevy render architecture: https://bevy.org/news/bevy-0-6/
- Bevy render systems API: https://docs.rs/bevy/latest/bevy/render/enum.RenderSystems.html
- Filament repository and API examples: https://github.com/google/filament
- Godot architecture diagram: https://docs.godotengine.org/en/stable/engine_details/architecture/godot_architecture_diagram.html
- Godot renderers overview: https://docs.godotengine.org/en/latest/tutorials/rendering/renderers.html
- bgfx overview: https://bkaradzic.github.io/bgfx/overview.html
- Falcor repository: https://github.com/NVIDIAGameWorks/Falcor
