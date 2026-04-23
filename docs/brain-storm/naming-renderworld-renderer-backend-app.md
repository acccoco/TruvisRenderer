# 名词辨析：RenderWorld / Renderer / RenderBackend / App

基于 `split-render-context-world-renderworld` 重构完成后的架构现状，
对引擎核心四个概念的内涵、边界、当前命名问题和改进方向做系统梳理。

## 1. 四个概念的本质定义

### RenderWorld — "GPU 侧有什么"

```
本质：数据容器 (plain struct, 无方法)
职责：聚合全部 GPU 侧状态的所有权
边界：只回答 "GPU 侧当前的状态是什么"
      不回答 "怎么推进一帧" 或 "何时提交命令"

包含：GpuScene, BindlessManager, FifBuffers, FrameCounter,
      GlobalDescriptorSets, GfxResourceManager, SamplerManager,
      PipelineSettings, FrameSettings, AccumData, per_frame_data_buffers

类比：Bevy 的 RenderWorld（渲染侧 ECS world）
定义位置：truvis-render-interface/src/render_world.rs
```

设计特征：
- **纯数据**，保持 plain struct + pub 字段，利用 Rust disjoint field borrowing
- 不持有执行能力（无 begin_frame / submit / present 等方法）
- 与 CPU 场景状态 (`World`) 物理分离

### Renderer — "怎么驱动 GPU"（当前实际是 Backend 角色）

```
本质：执行引擎 (有方法，有副作用)
职责：帧生命周期执行 — begin_frame / submit / present / end_frame
      GPU 数据上传、descriptor 更新、swapchain 管理
边界：知道 "如何" 完成一个 GPU 操作
      不知道 "何时" 或 "以什么顺序" 调用自己的方法

持有：World + RenderWorld + CmdAllocator + Timer + Semaphore + RenderPresent

定义位置：truvis-renderer/src/renderer.rs
```

关键特征：
- 被 FrameRuntime 驱动，自身不做调度决策
- 同时持有 CPU 状态 (World) 和 GPU 状态 (RenderWorld) 的所有权
- doc comment 自称 "渲染 Backend 核心"

### FrameRuntime — "什么时候做什么"

```
本质：编排器 (orchestrator / scheduler)
职责：帧 phase 排序、输入分发、UI 宿主、plugin 生命周期管理
边界：知道 "先 update 再 render 再 present" 的顺序
      不知道 GPU 具体怎么提交命令

持有：Renderer + CameraController + InputManager + GuiHost + Plugin + Overlays

定义位置：truvis-frame-runtime/src/frame_runtime.rs
```

调用顺序：
```
begin_frame → input → acquire → build_ui → update → asset_upload
→ gpu_scene_update → render_graph_build_and_execute → present → end_frame
```

### App (AppPlugin) — "画什么、怎么交互"

```
本质：用户逻辑插件
职责：场景搭建、RenderGraph 构建、UI 交互
边界：通过 typed contexts 访问引擎能力
      不接触 begin_frame / end_frame / submit / present

只看到：InitCtx, UpdateCtx, RenderCtx, ResizeCtx
      （精心裁剪过的视图窗口）

定义位置：truvis-app-api/src/app_plugin.rs (trait)
实现位置：truvis-app/src/outer_app/*.rs (cornell, sponza, triangle, shader_toy)
```

## 2. 职责矩阵

```
              数据所有权    执行能力    调度决策    用户逻辑
              ─────────    ────────    ────────    ────────
RenderWorld      ✓           ✗           ✗           ✗
Renderer         ✓(间接)     ✓           ✗           ✗
FrameRuntime     ✓(间接)     ✓(间接)     ✓           ✗
App              ✗           ✓(受限)     ✗           ✓
```

```
              知道GPU细节?   知道帧顺序?   知道场景语义?
              ───────────   ──────────   ───────────
RenderWorld      ✓              ✗             ✗
Renderer         ✓              ✗             ✗  (只做上传)
FrameRuntime     ✗  (委托)      ✓             ✗
App              ✗  (通过ctx)   ✗  (被调用)    ✓
```

## 3. 架构关系图

```
┌─────────────────────────────────────────────────────────────┐
│                     ENGINE RUNTIME STACK                     │
│                                                             │
│   ┌──────────────┐                                          │
│   │  App(Plugin)  │  "What to draw & how the user           │
│   │  cornell_app  │   experiences it"                       │
│   │  sponza_app   │  Scene setup, RG build, interaction     │
│   └──────┬───────┘                                          │
│          │ uses contexts (InitCtx, RenderCtx...)            │
│          ▼                                                  │
│   ┌──────────────────┐                                      │
│   │  FrameRuntime    │  "When things happen"                │
│   │  (orchestrator)  │  Phase ordering, input, UI host      │
│   └──────┬───────────┘                                      │
│          │ drives                                           │
│          ▼                                                  │
│   ┌──────────────────┐                                      │
│   │    Renderer      │  "How GPU work gets executed"        │
│   │ (actual role:    │  begin/end frame, submit, present,   │
│   │  RenderBackend)  │  swapchain, cmd alloc, sync          │
│   └──┬──────────┬────┘                                      │
│      │ owns     │ owns                                      │
│      ▼          ▼                                           │
│  ┌────────┐ ┌──────────────┐                                │
│  │ World  │ │ RenderWorld  │  "What state exists"           │
│  │ (CPU)  │ │ (GPU)        │  Pure data containers,         │
│  └────────┘ └──────────────┘  no execution logic            │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 4. 当前命名问题

### 问题 1：Renderer 名不副实

`Renderer` 的 doc comment 写的是 "渲染 Backend 核心"，但 struct 名叫 `Renderer`。

在行业惯例中，`Renderer` 通常暗示 "负责渲染的完整系统"——既有执行能力又有调度能力。
但我们的 `Renderer` 是一个**被动的 backend**，调度权完全在 `FrameRuntime`。

```
行业预期：Renderer = 执行 + 调度（完整渲染管线）
实际情况：Renderer = 仅执行（被 FrameRuntime 驱动的 backend）
```

### 问题 2：truvis-render-interface 名不副实

`render-interface` 暗示 trait / 契约 / 抽象边界（类似 Java 的 interface），
但实际包含的是大量具体 Manager 实现和 `RenderWorld` 聚合容器。

```
名字暗示：traits, type definitions, thin contracts
实际内容：BindlessManager, GpuScene, GfxResourceManager, FifBuffers,
          GlobalDescriptorSets, SamplerManager, CmdAllocator ...
          全是有状态的具体实现
```

Cargo.toml 的 description "GPU 的边界" 比 crate 名更贴切。

### 问题 3：FrameRuntime 与 Renderer 的关系不透明

FrameRuntime 总是 1:1 持有 Renderer，外部代码（truvis-winit-app）
只接触 FrameRuntime，永远不直接操作 Renderer。
从外部视角看，FrameRuntime 才是 "那个 Renderer"。

```
外部视角：FrameRuntime 就是引擎 = "Renderer"
内部视角：FrameRuntime 是调度，Renderer 是执行
```

## 5. 什么时候用哪个名字

| 讨论场景 | 应使用的概念 | 原因 |
|---|---|---|
| GPU 侧有哪些 buffer / descriptor / 帧设置 | **RenderWorld** | 纯数据，无行为 |
| 怎么提交命令、管理 swapchain、做 GPU 上传 | **Renderer** (实为 Backend) | 执行能力，不做调度 |
| 帧循环顺序、输入分发、plugin 生命周期 | **FrameRuntime** | 调度决策 |
| 用户场景搭建、渲染管线选择、UI 交互 | **App** (AppPlugin) | 用户逻辑 |
| 泛指 "渲染子系统" | **Renderer** | 最广为人知的行业术语 |

## 6. 改进方向

### 方案 A：最小改动 — 只修正 truvis-render-interface 命名

```
truvis-render-interface  →  truvis-render-core

理由：crate 内容是渲染核心基础设施（managers + RenderWorld），
      "core" 比 "interface" 准确得多。
影响：~35 个 .rs 文件 import 路径 + ~12 个 Cargo.toml
```

### 方案 B：中等改动 — 同时修正 Renderer 命名

```
truvis-render-interface  →  truvis-render-core
truvis-renderer (struct Renderer)  →  truvis-render-backend (struct RenderBackend)

理由：消除 "Renderer" 一词的歧义。
      doc comment 已经自称 "Backend 核心"，让名字跟上。
影响：额外 ~10 个 .rs 文件 + 上下游 Cargo.toml
```

此方案下 `Renderer` 这个词要么：
- 空出来，留给未来 "FrameRuntime + Backend" 的完整组合
- 不再用作 struct 名，仅作口语称呼

### 方案 C：大改动 — 合并 FrameRuntime + Renderer

```
FrameRuntime + Renderer  →  合并为新的 Renderer
原 Renderer 内部逻辑降级为 private 实现细节

结构变为：
  Renderer（= 当前 FrameRuntime 的外壳 + 当前 Renderer 的内脏）
    ├─ World
    ├─ RenderWorld
    ├─ CmdAllocator, Timer, Sync ...
    ├─ CameraController, InputManager, GuiHost
    └─ Plugin
```

理由：外部已经只通过 FrameRuntime 交互，合并后 API 更简洁。
风险：职责边界模糊化，文件体量增大，需要谨慎的内部分层。

### 推荐路径

```
短期：方案 A（改 truvis-render-interface → truvis-render-core）
      性价比最高，消除最显眼的命名误导

中期：方案 B（Renderer → RenderBackend）
      配合 crate 重命名一起做，让命名体系自洽

远期：评估方案 C
      等 tick system / multi-pipeline 等重构完成后再考虑，
      届时 FrameRuntime 和 Renderer 的边界可能自然演化
```

## 7. 对 truvis-render-interface 重命名候选的详细评估

| 候选名 | 含义 | 优点 | 缺点 |
|---|---|---|---|
| `truvis-render-core` | 渲染核心基础设施 | 简短、业界常用、准确 | "core" 一词有时被滥用 |
| `truvis-render-infra` | 渲染基础设施 | 精确描述实际内容 | 略显非正式 |
| `truvis-render-state` | 渲染状态管理 | 突出 RenderWorld 数据容器角色 | 低估了 Manager 的执行逻辑 |
| `truvis-render-base` | 渲染基座 | 简单直白 | 语义模糊 |
| `truvis-render-substrate` | 渲染基底 | 精确、有区分度 | 不常见，学习成本 |
| `truvis-gpu-runtime` | GPU 运行时 | 强调 GPU 侧 | 与 FrameRuntime 命名冲突 |

综合推荐：**`truvis-render-core`**
- 业界最广泛接受（Bevy 用 `bevy_render`，wgpu 生态用 `*-core`）
- 准确传达 "渲染子系统的核心层" 含义
- 与 `truvis-gfx`（RHI）和 `truvis-renderer`（backend）形成清晰层次
