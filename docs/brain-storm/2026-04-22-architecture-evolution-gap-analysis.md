# 架构演进差距分析（2026-04-22）

> 四次架构重构完成后的现状盘点。对照理想架构（App / World / RenderWorld / Plugin），
> 找出已完成、可调整、待探索的方向。
>
> 前序参考：
> - [ideal_layered_architecture.md](ideal_layered_architecture.md) — Bevy 风格理想分层
> - [render-app-layering-analysis.md](render-app-layering-analysis.md) — RenderApp 诊断
> - [plugin-pass-eventbus-evolution.md](plugin-pass-eventbus-evolution.md) — Plugin/Pass 演化
> - [plugin-imgui-winit-multi-pipeline-integration.md](plugin-imgui-winit-multi-pipeline-integration.md) — 多管线集成

---

## 1. 已完成的重构回顾

| 时间 | 变更 | 核心成果 |
|------|------|---------|
| 04-11 | clean-crate-dependencies | render-graph 不再依赖 scene/asset；gui-backend 不再依赖 render-graph；依赖方向清理 |
| 04-18 | render-thread-isolation | 渲染线程独立；winit 主线程只做事件泵；crossbeam channel 通信 |
| 04-20 | frame-runtime-boundary-refactor | RenderApp → FrameRuntime；OuterApp → AppPlugin；显式 phase 替代 big\_update |
| 04-20 | runtime-api-crate-split | truvis-app-api / truvis-frame-runtime / truvis-render-passes 三个新 crate；typed contexts |

这些重构把系统从"单体 RenderApp + 全透穿 RenderContext"推进到了"FrameRuntime 调度 + AppPlugin 契约 + 物理 crate 分层"。

---

## 2. 当前架构总览

```
truvis-winit-app (L6 — Platform Entry)
    │
    ▼
FrameRuntime (truvis-frame-runtime, L5)
    ├── CameraController          ← 硬编码
    ├── InputManager              ← 硬编码
    ├── GuiHost                   ← 硬编码
    ├── Vec<OverlayModule>        ← 硬编码
    ├── Box<dyn AppPlugin>        ← 单插件槽位
    │
    └── Renderer (truvis-renderer, L4)
        ├── CmdAllocator
        ├── Timer
        ├── FIF timeline semaphore
        ├── RenderPresent (swapchain)
        │
        └── RenderContext              ← 单一大袋子
            ├── SceneManager           ┐
            ├── AssetHub               │ CPU World 的内容
            ├── FrameSettings          │
            ├── PipelineSettings       ┘
            ├── GpuScene               ┐
            ├── BindlessManager        │ Render World 的内容
            ├── GlobalDescriptorSets   │
            ├── FifBuffers             │
            ├── GfxResourceManager     │
            ├── SamplerManager         ┘
            ├── FrameCounter
            ├── AccumData
            └── delta_time_s / total_time_s
```

### 帧内阶段（FrameRuntime.run_frame）

```
begin_frame ─► phase_input ─► phase_update ─► phase_prepare ─► phase_render ─► phase_present
     │              │              │                │                │               │
  renderer      GuiHost +      acquire +          GPU scene      plugin.render   present +
  begin_frame   InputManager   build_ui +         upload +                       end_frame
  + update_     process        camera +           before_render
    assets                     plugin.update      (internal, no
                                                   plugin hook)
```

---

## 3. 理想架构对照

理想的四层模型：

```
┌───────────────────────────────────────────────────────────────┐
│ App                                                           │
│   调度 / 生命周期 / Phase 编排                                  │
│                                                               │
│   ┌──────────────────────┐    ┌──────────────────────────┐    │
│   │ World (CPU)          │    │ RenderWorld (GPU)         │    │
│   │  SceneManager        │    │  GpuScene                 │    │
│   │  AssetHub            │    │  BindlessManager          │    │
│   │  Camera / Input      │    │  GlobalDescriptorSets     │    │
│   │  Time / Settings     │    │  FifBuffers               │    │
│   │                      │    │  Passes / Pipelines       │    │
│   └──────────┬───────────┘    └──────────▲────────────────┘    │
│              │                           │                     │
│              └─── extract (RenderData) ──┘                     │
│                                                               │
│   Plugins: [InputPlugin, CameraPlugin, GuiPlugin,             │
│             ScenePlugin, UserPlugin, ...]                      │
└───────────────────────────────────────────────────────────────┘
```

---

## 4. 差距分析

### 4.1 RenderContext 是 World + RenderWorld 的混合体 ★★★

**现状**：`RenderContext` 持有 CPU 场景数据（`SceneManager`、`AssetHub`）和 GPU 渲染状态（`GpuScene`、`BindlessManager`、`GlobalDescriptorSets`、`FifBuffers`）。所有 plugin context 类型（`InitCtx`、`UpdateCtx`、`RenderCtx`、`ResizeCtx`）都是从这个大袋子里借出不同切片。

**差距**：没有 World / RenderWorld 的物理分离。CPU 状态的修改和 GPU 状态的修改发生在同一个所有权域中，无法独立演化。

**已有基础**：`prepare_render_data()` → `RenderData` → `upload_render_data()` 这条路径本质上就是 extract。只是它被藏在 `Renderer::before_render` 里，没有作为架构概念显式存在。

**建议的拆分方向**：

```rust
struct World {
    scene_manager: SceneManager,
    asset_hub: AssetHub,
    camera: Camera,
    input_state: InputState,
    timer: Timer,
    frame_settings: FrameSettings,
    pipeline_settings: PipelineSettings,
}

struct RenderWorld {
    gpu_scene: GpuScene,
    bindless_manager: BindlessManager,
    global_descriptor_sets: GlobalDescriptorSets,
    fif_buffers: FifBuffers,
    gfx_resource_manager: GfxResourceManager,
    sampler_manager: RenderSamplerManager,
    frame_counter: FrameCounter,
    accum_data: AccumData,
}
```

### 4.2 FrameRuntime 硬编码了应该是 Plugin 的东西 ★★

**现状**：`CameraController`、`InputManager`、`GuiHost`、`Vec<OverlayModule>` 都直接作为 `FrameRuntime` 的字段。

**差距**：理想架构中，这些都是可选的 Plugin，用户可以替换或移除。

**约束**：Camera、Input、GUI 与帧阶段有固定的时序依赖（input 必须在 update 前、GUI 必须在 render 前），不能无条件地做成"任意顺序 plugin"。

**可选演化路径**：
- **小步**：保持 FrameRuntime 拥有这些组件，但通过 trait 抽象允许替换实现
- **中步**：引入 `BuiltinPlugin` 概念，Camera/Input/GUI 是默认注册的 BuiltinPlugin
- **大步**：完整 Plugin 容器 + Phase schedule（类 Bevy）

### 4.3 Plugin 系统是单槽位，缺乏组合能力 ★★

**现状**：`Box<dyn AppPlugin>` — 一个 runtime 只有一个 plugin 实例。

**差距**：无法组合多个独立 plugin（例如一个处理 RT，一个处理 post-processing，一个处理 debug overlay）。

**已有基础**：`OverlayModule` 已经是一种"mini plugin"模式（`Vec<Box<dyn OverlayModule>>`），说明多组件注册的需求真实存在。

**建议**：

```rust
// Phase 1: 多 plugin 列表
struct App {
    plugins: Vec<Box<dyn Plugin>>,
}

trait Plugin {
    fn build(&self, app: &mut AppBuilder);    // 注册资源和子 plugin
    fn init(&mut self, world: &mut World);
    fn update(&mut self, world: &mut World);
    fn extract(&self, world: &World, render_world: &mut RenderWorld);
    fn render(&self, render_world: &RenderWorld);
    fn on_resize(&mut self, world: &mut World, render_world: &mut RenderWorld) {}
    fn shutdown(&mut self) {}
}
```

### 4.4 Renderer 混合了 Backend 和调度逻辑 ★

**现状**：`Renderer` 既做 GPU 后端操作（设备、命令、FIF 同步、present），又做数据调度（`update_assets`、`update_gpu_scene`、`update_perframe_descriptor_set`）。

**差距**：理想中 Renderer 只是 GPU Backend，extract/prepare 逻辑属于 App 的调度层。

**已有基础**：`FrameRuntime` 已经在控制调用顺序（`phase_prepare` 调用 `renderer.before_render()`），只是 `before_render` 内部仍然是 Renderer 自己在编排。

### 4.5 Extract 没有作为显式 Phase 存在 ★

**现状**：`prepare_render_data()` + `upload_render_data()` 藏在 `Renderer::before_render` 中，从 `FrameRuntime::phase_prepare` 调用。

**差距**：Extract 应该是一个独立的、plugin 可参与的 phase。

**已有数据流**：

```
SceneManager.prepare_render_data()   ← CPU snapshot (extract)
        │
        ▼
    RenderData<'a>                   ← 中间表示
        │
        ▼
GpuScene.upload_render_data()        ← GPU upload (prepare)
```

只需把这条路径从 `Renderer::before_render` 中提取出来，暴露为 `phase_extract` 即可。

---

## 5. 演化优先级

| 优先级 | 方向 | 描述 | 前置 | 影响 crate |
|--------|------|------|------|------------|
| **P0** | 拆分 RenderContext → World + RenderWorld | 建立 CPU/GPU 状态的物理分离 | 无 | renderer, app-api, frame-runtime |
| **P1** | Extract 提升为显式 Phase | 从 before_render 中分离出 phase_extract | P0 | renderer, frame-runtime |
| **P1** | 多 Plugin 支持 | `Box<dyn AppPlugin>` → `Vec<Box<dyn Plugin>>` | 无（可并行） | app-api, frame-runtime, app |
| **P2** | Camera/Input/GUI Plugin 化 | 从 FrameRuntime 字段变成可替换的 Plugin | P1 | frame-runtime |
| **P2** | Renderer 职责收窄 | 把 extract/prepare 调度移出 Renderer | P0+P1 | renderer, frame-runtime |
| **P3** | Plugin 依赖声明 | Plugin 之间可声明执行顺序和依赖 | P1 | app-api |

### P0 拆分 RenderContext 的初步思路

**Step 1**: 在 `truvis-render-interface` 中定义 `World` 和 `RenderWorld` 结构

```
truvis-render-interface (已有 GpuScene、RenderData 等)
  ├── world.rs         // World struct
  └── render_world.rs  // RenderWorld struct
```

**Step 2**: `RenderContext` 改为持有 `World` + `RenderWorld`（过渡期）

```rust
pub struct RenderContext {
    pub world: World,
    pub render_world: RenderWorld,
}
```

**Step 3**: Plugin contexts 改为借用 `World` / `RenderWorld`

```rust
pub struct UpdateCtx<'a> {
    pub world: &'a mut World,        // CPU 可写
}

pub struct RenderCtx<'a> {
    pub render_world: &'a RenderWorld,  // GPU 只读
    pub present: &'a RenderPresent,
    pub draw_data: &'a imgui::DrawData,
}
```

**Step 4**: 删除 `RenderContext`，`Renderer` 只持有 Backend 资源 + `RenderWorld`

---

## 6. 与现有 brain-storm 的关系

本文是以下文档的**实施续篇**：

- `ideal_layered_architecture.md` 给出了 Bevy 风格的理想分层 → 本文确认理想方向，聚焦"当前离理想多远"
- `render-app-layering-analysis.md` 诊断了 RenderApp 混合体问题 → 四次重构已解决大部分，但 RenderContext 混合体问题仍在
- `plugin-pass-eventbus-evolution.md` 提出了多 plugin 演化路线 → 本文将其定为 P1
- `plugin-imgui-winit-multi-pipeline-integration.md` 讨论了 ImGui/多管线集成 → 对应 P2 GUI Plugin 化

---

## 7. 关于你的理想架构观点的评估

**App / World / RenderWorld / Plugin 四层模型方向正确**，以下是几个需要注意的决策点：

### 7.1 World 的抽象程度

Bevy 的 `World` 是通用 ECS 容器（任意 Component/Resource），这对于渲染引擎来说可能**过度抽象**。建议：

- **不做通用 ECS**：你的 World 可以是类型化的结构体（`SceneManager` + `AssetHub` + `Camera` + ...）
- 如果未来有场景编辑器或游戏逻辑需求，再考虑引入 `TypeMap` 或轻量 ECS
- 关键价值不是"通用容器"，而是"CPU 状态和 GPU 状态有明确的所有权边界"

### 7.2 Extract 的粒度

Bevy 的 extract 是 per-system 的（每个 render system 各自 extract）。你当前的模式是全场景一次性 extract（`prepare_render_data` → `RenderData`）。对于渲染引擎：

- **全场景 extract 更合理** — GPU 上传通常是 batch 操作
- 但要注意 TLAS "build once per FIF slot" 的限制（动态场景需要解决 rebuild 问题）
- 未来如果需要细粒度 extract（例如仅 dirty material），可以在全场景 extract 内部做增量

### 7.3 Plugin 的调度模型

不需要走 Bevy 的完整 Schedule 系统（`SystemSet`、`Condition`、`State` 等）。对于渲染引擎：

- **固定 Phase + 有序 Plugin 列表** 足够实用
- Phase: `init → input → update → extract → prepare → render → present`
- 每个 Plugin 在每个 Phase 有可选钩子
- 如果 Plugin 之间有顺序依赖，用显式声明（`after: ["scene"]`）而非自动推导

### 7.4 不要一步到位

当前架构已经比重构前健康很多。建议**逐步演化**，每步都是可运行的：

```
当前状态
  │
  ▼  P0: 拆分 RenderContext
World + RenderWorld 在同一个 Renderer 中共存
  │
  ▼  P1: Extract Phase + 多 Plugin
FrameRuntime 有显式 extract 阶段，支持多 plugin
  │
  ▼  P2: Camera/Input/GUI Plugin 化
FrameRuntime 变薄，大部分逻辑是 Plugin
  │
  ▼  P3: Renderer 收窄
Renderer 只做 Backend，调度完全在 App/Runtime
  │
  ▼  理想状态
App / World / RenderWorld / Plugin 四层清晰
```

每一步都可以独立实施、独立验证、独立回滚。
