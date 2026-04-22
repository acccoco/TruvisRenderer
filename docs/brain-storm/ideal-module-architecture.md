# 理想模块架构：组件级分析

> 日期：2026-04-17
> 状态：探索 / 方案设计
> 前置：[clean-crate-dependencies.md](clean-crate-dependencies.md) 中完成的 Layer 3 解耦

## 一、背景

[上一次依赖清理](clean-crate-dependencies.md) 解决了 render-graph 向上依赖 scene/asset 的问题，但仍有若干架构纠结未处理。本文在 crate 内部**组件级别**（SceneManager、BindlessManager、GpuScene 等）进行分析，给出理想的组件归属和依赖关系。

## 二、现状：组件清单与归属

### truvis-gfx（Layer 1）

| 组件 | 职责 |
|------|------|
| `Gfx` | Vulkan 单例，设备/队列/VMA |
| `GfxCommandBuffer` | 命令录制 |
| `GfxImage` / `GfxBuffer` / `GfxImageView` | GPU 资源对象 |
| `GfxSemaphore` | 同步原语 |
| `GfxAcceleration` | RT 加速结构 |
| `GfxStructuredBuffer<T>` | 类型化 SSBO |
| `GfxSwapchain` | 交换链 |

无需变动。

### truvis-render-interface（Layer 2）

| 组件 | 职责 | 问题 |
|------|------|------|
| `Handles` (GfxImageHandle, GfxBufferHandle, GfxImageViewHandle) | 轻量级资源标识 | ✓ |
| `FrameCounter` / `FrameLabel` / `FrameToken` | 帧计数与 FIF 管理 | ✓ |
| `GfxResourceManager` | SlotMap 资源池 + 延迟销毁 | ✓ |
| `CmdAllocator` | per-FIF 命令池 | ✓ |
| `StageBufferManager` | staging buffer 分配 | ✓ |
| `BindlessManager` | bindless descriptor 管理 | ✓ |
| `GlobalDescriptorSets` | Set 0/1/2 布局与分配 | ✓ |
| `RenderSamplerManager` | 全局 sampler 写入 Set 0 | ✓ |
| `PipelineSettings` / `FrameSettings` | 渲染参数 | ✓ |
| `RtGeometry` | SoA 顶点 + 索引 + BLAS 输入 | ✓ |
| **`GpuScene`** | GPU 场景 buffer + TLAS | **✗ 场景语义，不属于资源管理层** |
| **`RenderData`** | CPU→GPU 场景快照 | **✗ 场景语义，不属于资源管理层** |

### truvis-render-graph（Layer 3a）

| 组件 | 职责 |
|------|------|
| `RenderGraphBuilder` / `CompiledGraph` | pass 图构建与执行 |
| `RgPass` (trait) / `RgPassBuilder` / `RgPassContext` | pass 接口 |
| `RgImageHandle` / `RgBufferHandle` / `RgImageState` | 虚拟资源句柄 |
| `ComputePass<P>` | 通用 compute pass |
| `FifBuffers` | per-FIF render target 管理 |
| `DependencyGraph` | 拓扑排序与依赖分析 |

无需变动。

### truvis-asset（Layer 3a）

| 组件 | 职责 | 问题 |
|------|------|------|
| `AssetHub` | 统一资产注册表 | **△ update() 内直接调 BindlessManager::register_srv()** |
| `AssetLoader` | IO 线程异步加载 | ✓ |
| `AssetUploadManager` | staging → GPU 上传 | ✓ |
| `AssetTextureHandle` / `LoadStatus` | 纹理资产句柄与状态 | ✓ |

### truvis-scene（Layer 3b，依赖 asset）

| 组件 | 职责 | 问题 |
|------|------|------|
| `SceneManager` | 场景对象 CRUD + SlotMap | **✗ prepare_render_data() 依赖 BindlessManager + AssetHub** |
| `MaterialManager` | 管理材质 GPU buffer | **△ 导入 AssetTextureHandle；TextureResolver trait 方向正确** |
| `MeshManager` | 管理 mesh + BLAS 构建 | ✓ |
| `Mesh` / `Material` / `Instance` | 场景组件 | ✓ |
| `Shapes` (CubeSoA, TriangleSoA, ...) | 几何图元 | ✓ |

### truvis-gui-backend（Layer 3a）

| 组件 | 职责 |
|------|------|
| `GuiBackend` | imgui 字体上传 + per-frame mesh 准备 |
| `GuiPass` | imgui DrawData → Vulkan 命令录制 |
| `GuiMesh` | imgui 顶点/索引 buffer |

无需变动。

### truvis-renderer（Layer 4）

| 组件 | 职责 | 问题 |
|------|------|------|
| `Renderer` | 帧生命周期管理 | ✓ |
| `RenderContext` / `RenderContext2` | 全局状态聚合 | **△ God Object，15+ 字段** |
| `RenderPresent` | 交换链 acquire / present | ✓ |
| `Camera` / `Timer` | 平台工具 | ✓ |
| `AssimpSceneLoader` | 模型文件加载 | ✓ |

### truvis-app（Layer 5）

| 组件 | 职责 | 问题 |
|------|------|------|
| `RenderApp` | 顶层应用循环 | ✓ |
| `OuterApp` (trait) | 应用开发者接口 | ✓ |
| `GuiHost` | imgui 前端 (NewFrame / Render) | ✓ |
| `GuiRgPass` | gui-backend → render-graph 桥梁 | **△ 更适合放 renderer** |
| `InputManager` / `CameraController` | 输入处理 | ✓ |
| `RtPipeline` | RT 渲染管线组装 | ✓ |
| 各种 Pass (Phong, RT, Accum, Blit, ...) | 具体渲染 pass | **△ 直接依赖 gfx / render-interface / render-graph** |

---

## 三、问题总结

### 3.1 render-interface 混入场景语义

`GpuScene` 和 `RenderData` 的核心操作是"将场景数据翻译成 GPU buffer"，需要同时访问 `SceneManager` 的输出和 `BindlessManager` / `GfxResourceManager` 的能力。这种跨域桥接逻辑属于集成层（renderer），不属于资源管理层。

### 3.2 SceneManager 穿透到 GPU 绑定层

`prepare_render_data()` 签名：

```rust
pub fn prepare_render_data(&self,
    bindless_manager: &BindlessManager,  // ← render-interface
    asset_hub: &AssetHub,                // ← asset
) -> RenderData
```

Scene 层在逻辑上只应关心"场景是什么"，但当前它需要知道"GPU 绑定如何工作"和"纹理资产如何查询"。

### 3.3 AssetHub 直接注册 SRV

`AssetHub::update()` 在纹理 Ready 时直接调用 `BindlessManager::register_srv()`，让 asset 层耦合了 bindless 实现细节。

### 3.4 app 跨层依赖过多

app 直接依赖 6 个 truvis crate：renderer、render-graph、gui-backend、gfx、render-interface、shader-binding。理想情况下只需依赖 renderer。

### 3.5 RenderContext 是 God Object

`RenderContext` 有 15+ 个 pub 字段，任何 pass 通过它都能访问一切，无法从类型层面约束 pass 的最小访问权限。

---

## 四、理想架构

### 4.1 分层总览

```
╔═══════════════════════════════════════════════════════════════════════════╗
║  Layer 5: truvis-app                                                     ║
║                                                                          ║
║  RenderApp, OuterApp (trait)                                             ║
║  GuiHost, InputManager, CameraController                                 ║
║  RtPipeline, 各种 Concrete Pass                                          ║
║                                                                          ║
║  依赖: 只依赖 renderer (通过 re-export 获得 render-graph / gfx 类型)      ║
╠══════════════════════════════════════════════════════════════════════════╣
║  Layer 4: truvis-renderer                                                ║
║                                                                          ║
║  Renderer, RenderPresent, Camera, Timer                                  ║
║  RenderContext (视图化) ← 聚合所有子系统                                  ║
║  SceneBridge (新) ← 场景→GPU 翻译                                        ║
║  GuiRgPass ← 从 app 移入                                                 ║
║  GpuScene, RenderData ← 从 render-interface 移入                         ║
║  AssimpSceneLoader                                                       ║
║                                                                          ║
║  re-export: RgPass, RgPassBuilder, GfxCommandBuffer, FrameLabel, ...    ║
╠══════════════════════════════════════════════════════════════════════════╣
║  Layer 3: Domain Modules (互不依赖)                                      ║
║                                                                          ║
║  render-graph          asset              scene           gui-backend    ║
║  RenderGraphBuilder    AssetHub           SceneManager    GuiBackend     ║
║  CompiledGraph         AssetLoader        MaterialManager GuiPass        ║
║  RgPass (trait)        AssetUploadMgr     MeshManager     GuiMesh        ║
║  ComputePass           AssetTextureHandle Mesh/Material                  ║
║  FifBuffers            LoadStatus         Instance                       ║
║                                           Shapes                         ║
║                                                                          ║
║  scene 不再依赖 asset，不再依赖 BindlessManager                           ║
║  asset 不再直接注册 SRV                                                   ║
╠══════════════════════════════════════════════════════════════════════════╣
║  Layer 2: truvis-render-interface (精简)                                  ║
║                                                                          ║
║  Handles, FrameCounter, FrameLabel, FrameToken                           ║
║  GfxResourceManager, CmdAllocator, StageBufferManager                    ║
║  BindlessManager, GlobalDescriptorSets, RenderSamplerManager             ║
║  PipelineSettings, FrameSettings, RtGeometry                             ║
║                                                                          ║
║  移走: GpuScene → renderer, RenderData → renderer                        ║
╠══════════════════════════════════════════════════════════════════════════╣
║  Layer 1: truvis-gfx                                                     ║
║  Layer 0: utils, logs, path, shader-binding, descriptor-layout           ║
╚══════════════════════════════════════════════════════════════════════════╝
```

### 4.2 组件依赖关系图

用 `→` 表示"使用/依赖"：

```
┌─────────────────────── Layer 5: app ───────────────────────┐
│                                                             │
│  RenderApp ──→ Renderer                                     │
│  OuterApp (trait) ──→ Renderer, Camera                      │
│  CameraController ──→ Camera, InputState                    │
│  Concrete Passes ──→ (通过 renderer re-export 获得:         │
│                       RgPass, RgPassBuilder, RgPassContext,  │
│                       GfxCommandBuffer, ComputePass,        │
│                       FrameLabel, GfxImageHandle, ...)      │
│                                                             │
│  零直接依赖: gfx, render-interface, render-graph,           │
│              gui-backend, asset, scene                       │
└─────────────────────────────┬───────────────────────────────┘
                              │ (仅依赖 renderer)
                              ▼
┌─────────────────────── Layer 4: renderer ──────────────────┐
│                                                             │
│  Renderer ──→ RenderPresent, RenderContext, SceneBridge,    │
│               Camera, Timer, FrameCounter, CmdAllocator,    │
│               GfxResourceManager                            │
│                                                             │
│  SceneBridge (新) ──→ SceneManager, AssetHub,               │
│                       BindlessManager, GpuScene, RenderData │
│                                                             │
│  GuiRgPass ──→ GuiPass (gui-backend), RgPass (render-graph),│
│                BindlessManager, GlobalDescriptorSets         │
│                                                             │
│  RenderContext ──→ 所有子系统（提供分层视图）                 │
│  GpuScene ──→ GfxResourceManager, BindlessManager,          │
│               GfxStructuredBuffer                           │
│  RenderData ──→ RtGeometry, BindlessSrvHandle               │
│                                                             │
│  AssimpSceneLoader ──→ SceneManager, AssetHub               │
└──┬──────────┬──────────┬──────────┬─────────────────────────┘
   │          │          │          │
   ▼          ▼          ▼          ▼
┌────────┐┌────────┐┌──────────┐┌───────────┐
│ scene  ││ asset  ││render-   ││gui-backend│  Layer 3
│        ││        ││graph     ││           │
│Scene   ││ Asset  ││ RgPass   ││ GuiPass   │
│ Manager││ Hub    ││ Compute  ││ GuiMesh   │
│ Mat Mgr││ Loader ││ Pass     ││ Gui       │
│ Mesh   ││ Upload ││ FIF      ││ Backend   │
│ Mgr    ││ Mgr    ││ Buffers  ││           │
└───┬────┘└───┬────┘└────┬─────┘└─────┬─────┘
    │         │          │            │
    └────┬────┘──────────┴────────────┘
         │ (全部只依赖 Layer 2 + Layer 1)
         ▼
┌──────────────────────────────────────┐
│  render-interface (精简)   Layer 2   │
│                                       │
│  Handles, FrameCounter, FrameLabel    │
│  GfxResourceManager, CmdAllocator     │
│  BindlessManager, GlobalDescriptorSets│
│  StageBufferManager, RenderSamplerMgr │
│  PipelineSettings, RtGeometry         │
└──────────────────┬───────────────────┘
                   │
                   ▼
          ┌──────────────┐
          │  gfx  Layer 1│
          └──────────────┘
```

### 4.3 组件依赖矩阵

行 = 组件，列 = 它使用的组件。`●` 保留，`○` 去除，`◆` 新增。

```
                  Gfx  ResMgr Bindless GlobDS FrmCnt AssetHub SceneMgr RenderData GpuScene RgPass
                  ───  ────── ──────── ────── ────── ──────── ──────── ────────── ──────── ──────
GfxResourceMgr     ●                          ●
CmdAllocator       ●                          ●
BindlessManager    ●    ●       ─       ●     ●
GlobalDescSets     ●                          ●
StageBufMgr        ●                          ●
SamplerMgr         ●                   ●

AssetHub           ●    ●       ○                           AssetHub 不再调 register_srv
AssetUploadMgr     ●
AssetLoader        (无 truvis 依赖)

SceneManager            (Handle 类型 + RtGeometry)          不再依赖 BindlessManager, AssetHub
MaterialManager    ●                          ●              TextureResolver (trait)
MeshManager        ●

RenderGraphBuilder ●    ●
CompiledGraph      ●    ●
ComputePass        ●                   ●     (FrameLabel)
FifBuffers         ●    ●       ●             ●

GuiPass            ●            ●      ●     (FrameLabel)
GuiBackend         ●    ●       ●             ●

SceneBridge (新)        ◆       ◆             ◆      ◆       ◆       ◆          ◆
GpuScene (→renderer)●   ●       ●             ●                      ●
RenderData               (Handle + RtGeometry + BindlessSrvHandle)
GuiRgPass (→renderer)                  ●     (FrameLabel)                                 ●
Renderer           ●    ●       ●      ●     ●      ●       ●                            
RenderPresent      ●    ●                    ●

RenderApp                                                                                  
OuterApp (trait)                                                                           
Concrete Passes    (全部通过 renderer re-export 访问)
```

---

## 五、三个核心变化

### 5.1 引入 SceneBridge，GpuScene / RenderData 上移到 renderer

**当前**：

```
SceneManager.prepare_render_data(bindless, asset_hub)
    → RenderData (含 BindlessSrvHandle)
        → GpuScene.upload(render_data)
```

scene 层直接解析 GPU 绑定：

```rust
// scene_manager.rs（当前）
let srv = bindless_manager.get_shader_srv_handle(
    asset_hub.get_texture_by_path(&mat.diffuse_map).view_handle
);
```

**理想**：

```
SceneManager.snapshot()
    → SceneSnapshot (纯逻辑数据，无 GPU 句柄)
        → SceneBridge.translate(snapshot, asset_hub, bindless)
            → RenderData (含 BindlessSrvHandle)
                → GpuScene.upload(render_data)
```

```rust
// scene_manager.rs（理想）- 零外部依赖
pub fn snapshot(&self) -> SceneSnapshot { ... }

pub struct SceneSnapshot<'a> {
    pub instances: Vec<InstanceSnapshot>,
    pub meshes: Vec<MeshSnapshot<'a>>,     // 引用 RtGeometry
    pub materials: Vec<MaterialSnapshot>,   // 只含路径和 PBR 参数
    pub point_lights: Vec<gpu::PointLight>,
}

pub struct MaterialSnapshot {
    pub diffuse_map: String,    // 路径，不是 GPU 句柄
    pub normal_map: String,
    pub metallic: f32,
    pub roughness: f32,
}
```

```rust
// scene_bridge.rs（新，在 renderer 中）
impl SceneBridge {
    pub fn update(
        &mut self,
        cmd: &GfxCommandBuffer,
        scene: &SceneManager,
        asset_hub: &AssetHub,
        bindless: &BindlessManager,
        frame_counter: &FrameCounter,
    ) {
        let snapshot = scene.snapshot();
        let render_data = self.translate(&snapshot, asset_hub, bindless);
        self.gpu_scene.upload(cmd, frame_counter, &render_data, bindless);
    }

    fn translate(
        &self,
        snapshot: &SceneSnapshot,
        asset_hub: &AssetHub,
        bindless: &BindlessManager,
    ) -> RenderData {
        // 集中处理: 路径 → AssetTextureHandle → view_handle → BindlessSrvHandle
    }
}
```

**涉及组件移动**：
- `GpuScene`：render-interface → renderer
- `RenderData`（含 `MaterialRenderData` 等）：render-interface → renderer
- `SceneManager::prepare_render_data()`：重构为 `snapshot()`
- `SceneBridge`：新建于 renderer

**效果**：
- scene 不再依赖 asset、BindlessManager
- render-interface 精简为纯资源管理
- 逻辑→物理的翻译集中在一处

### 5.2 SRV 注册从 AssetHub 上移到 renderer

**当前**：

```rust
// asset_hub.rs
pub fn update(&mut self, resource_mgr: &mut GfxResourceManager,
              bindless: &mut BindlessManager) {
    // ... 纹理上传完成后：
    bindless.register_srv(image_view_handle);
}
```

**理想**：AssetHub 只负责到"GfxImage 已创建 + ImageView 已分配"，返回状态变化通知。SRV 注册由 renderer 在帧更新阶段统一处理。

```rust
// asset_hub.rs（理想）
pub fn update(&mut self, resource_mgr: &mut GfxResourceManager)
    -> Vec<AssetReadyEvent>
{
    // 不再接收 bindless 参数
    // 返回新 Ready 的纹理列表
}

// renderer 中
fn begin_frame_assets(&mut self) {
    let ready = self.asset_hub.update(&mut self.resource_mgr);
    for event in ready {
        self.bindless_manager.register_srv(event.view_handle);
    }
}
```

**效果**：asset 与 render-interface 的 BindlessManager 解耦。

### 5.3 Renderer re-export + app 依赖收敛

**当前 app 依赖 6 个 crate**：

```toml
# truvis-app/Cargo.toml（当前）
truvis-renderer = { workspace = true }
truvis-render-graph = { workspace = true }
truvis-gui-backend = { workspace = true }
truvis-gfx = { workspace = true }
truvis-render-interface = { workspace = true }
truvis-shader-binding = { workspace = true }
# ...
```

**理想 app 只依赖 renderer**：

```toml
# truvis-app/Cargo.toml（理想）
truvis-renderer = { workspace = true }
truvis-shader-binding = { workspace = true }  # shader 类型绑定，可接受
```

renderer 通过 `pub use` 重导出 app 需要的类型：

```rust
// truvis-renderer/src/lib.rs

// render-graph 类型（app 写 pass 需要）
pub use truvis_render_graph::render_graph::{
    RenderGraphBuilder, CompiledGraph,
    RgPass, RgPassBuilder, RgPassContext,
    RgImageHandle, RgBufferHandle, RgImageState, RgSemaphoreInfo,
};
pub use truvis_render_graph::compute_pass::ComputePass;

// gfx 类型（app 偶尔需要的 Vulkan 原语）
pub use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
pub use truvis_gfx::commands::semaphore::GfxSemaphore;
pub use truvis_gfx::gfx::Gfx;

// render-interface 类型（句柄和帧标签）
pub use truvis_render_interface::handles::*;
pub use truvis_render_interface::frame_counter::{FrameCounter, FrameLabel};
pub use truvis_render_interface::global_descriptor_sets::GlobalDescriptorSets;
```

**效果**：app 的 Cargo.toml 只列 renderer + shader-binding，其余通过 re-export 使用。

---

## 六、RenderContext 视图化（渐进改进）

当前 `RenderContext` 有 15+ 个 pub 字段，任何 pass 都能访问一切。

**方案**：保留 `RenderContext` 作为内部存储，对外提供分层视图：

```rust
pub struct SceneView<'a> {
    pub gpu_scene: &'a GpuScene,
    pub scene_manager: &'a SceneManager,
}

pub struct GpuView<'a> {
    pub bindless_manager: &'a BindlessManager,
    pub global_descriptor_sets: &'a GlobalDescriptorSets,
    pub gfx_resource_manager: &'a GfxResourceManager,
    pub sampler_manager: &'a RenderSamplerManager,
}

pub struct FrameView<'a> {
    pub frame_counter: &'a FrameCounter,
    pub frame_settings: &'a FrameSettings,
    pub pipeline_settings: &'a PipelineSettings,
    pub delta_time_s: f32,
    pub total_time_s: f32,
}

impl RenderContext {
    pub fn scene_view(&self) -> SceneView<'_> { ... }
    pub fn gpu_view(&self) -> GpuView<'_> { ... }
    pub fn frame_view(&self) -> FrameView<'_> { ... }
}
```

Pass 签名从：

```rust
pub fn draw(&self, render_context: &RenderContext) { ... }
```

变为：

```rust
pub fn draw(&self, gpu: &GpuView, scene: &SceneView, frame: &FrameView) { ... }
```

可渐进推进：先加视图方法，逐步迁移各 pass。

---

## 七、理想执行时序

```
RenderApp::big_update()                                      [app]
│
├── 1. Renderer::begin_frame()                               [renderer]
│       ├── GfxSemaphore::wait_timeline (N-3)                [gfx]
│       ├── GfxResourceManager::cleanup()                    [render-interface]
│       └── FrameCounter::advance()                          [render-interface]
│
├── 2. InputManager::process_events()                        [app]
│       └── CameraController::update()                       [app]
│
├── 3. RenderPresent::acquire_next_image()                   [renderer]
│
├── 4. Scene + Asset 更新
│       ├── OuterApp::update()                               [app → scene]
│       ├── AssetHub::update() → Vec<AssetReadyEvent>        [asset]  ← 不再调 register_srv
│       ├── Renderer::register_new_assets(events)            [renderer]  ← 新: 统一 SRV 注册
│       │     └── BindlessManager::register_srv()            [render-interface]
│       ├── MaterialManager::update(texture_resolver)        [scene]
│       ├── MaterialManager::upload(cmd, frame_label)        [scene]
│       ├── SceneBridge::update()                            [renderer]  ← 新: 翻译 + 上传
│       │     ├── SceneManager::snapshot() → SceneSnapshot   [scene]
│       │     ├── SceneBridge::translate() → RenderData      [renderer]
│       │     └── GpuScene::upload()                         [renderer]
│       └── GlobalDescriptorSets::update()                   [render-interface]
│
├── 5. GUI 帧构建
│       ├── GuiHost::new_frame()                             [app]
│       │     └── OuterApp::draw_ui()                        [app]
│       ├── GuiHost::compile_ui()                            [app]
│       └── GuiBackend::prepare_render_data()                [gui-backend]
│
├── 6. Render Graph 构建与执行
│       ├── OuterApp::draw()                                 [app]
│       │     ├── RenderGraphBuilder::new()                  [render-graph]
│       │     ├── add_pass(gbuffer / lighting / rt / ...)    [render-graph]
│       │     └── add_pass(GuiRgPass)                        [renderer]
│       ├── RenderGraphBuilder::compile()                    [render-graph]
│       └── CompiledGraph::execute()                         [render-graph]
│
├── 7. Queue::submit() + RenderPresent::present()            [gfx + renderer]
│
└── 8. Renderer::end_frame()                                 [renderer]
```

---

## 八、改造优先级

| 优先级 | 改动 | 收益 | 成本 | 风险 |
|--------|------|------|------|------|
| **P0** | Renderer re-export + app 去掉直接依赖 | app 依赖从 6→1 | 低：只加 `pub use`，调 import 路径 | 极低 |
| **P1** | GpuScene / RenderData 移到 renderer | render-interface 精简，场景桥梁集中 | 中：移动代码 + 调整 import | 低 |
| **P2** | SceneManager 去掉 BindlessManager / AssetHub 依赖，引入 SceneBridge | 真正的层次隔离 | 中高：重构 prepare_render_data | 中（需仔细测试数据一致性） |
| **P2** | AssetHub 去掉 BindlessManager 依赖，SRV 注册上移 | asset 与 bindless 解耦 | 中：改 update 签名和调用方 | 低 |
| **P3** | RenderContext 视图化 | pass 最小权限可见 | 低→中：可渐进 | 极低 |
| **P3** | GuiRgPass 移到 renderer | 胶水逻辑归集成层 | 低 | 极低 |

P0 几乎零成本高收益，可以立即做。P1/P2 是真正的架构改善。P3 可以渐进推进。
