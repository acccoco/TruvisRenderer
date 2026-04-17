# Engine Crates

渲染引擎各功能模块，按依赖层次从底层到上层排列。

## 依赖层次

```
Layer 0 (Foundation)
├── truvis-utils
├── truvis-logs
├── truvis-path
├── truvis-shader-binding
└── truvis-descriptor-layout-*

Layer 1 (RHI)
└── truvis-gfx

Layer 2 (Resource Management)
└── truvis-render-interface

Layer 3 (Render Graph / Domain)    ← 同层，互不依赖
├── truvis-render-graph            ← 纯 pass 编排，不依赖 scene/asset
├── truvis-asset
├── truvis-scene
└── truvis-gui-backend             ← 纯 Vulkan 录制，不依赖 render-graph

Layer 4 (Integration)
└── truvis-renderer                ← 组装 RenderContext，整合 scene/asset/graph/gui

Layer 5 (App Framework)
└── truvis-app                     ← OuterApp trait + render pipeline + GuiRgPass 适配

Layer 6 (Binaries)
└── truvis-winit-app               ← 具体可执行文件
```

**核心依赖链（主干）：**

```
truvis-gfx
  └── truvis-render-interface
        └── truvis-render-graph
              └── truvis-renderer
                    └── truvis-app
                          └── truvis-winit-app
```

**领域模块（与 render-graph 同层，依赖 gfx + render-interface）：**

```
truvis-asset        ── gfx, render-interface
truvis-scene        ── gfx, render-interface, asset
truvis-gui-backend  ── gfx, render-interface（不依赖 render-graph）
```

---

## 模块说明

### `truvis-gfx`
Vulkan RHI 封装层，以 `Gfx` 单例提供设备、队列、内存分配器（VMA）等底层 GPU 资源的访问接口。所有上层模块的 Vulkan 调用均通过此层进行。

### `truvis-render-interface`
GPU 资源管理边界，包含：
- **`GfxResourceManager`**：基于 SlotMap 的资源池，管理 Image / Buffer / Sampler 等 GPU 资源，返回轻量级 Handle。
- **`CmdAllocator`**：按帧标签（A/B/C）分配和复用 CommandBuffer。
- **`FrameCounter`**：帧计数器，管理 Frames in Flight（固定 3 帧）。
- **`BindlessManager`** / **`GlobalDescriptorSets`**：全局三层 Bindless 绑定集（Set 0~2）。
- **`StageBufferManager`**：staging buffer 上传管理。

### `truvis-render-graph`
声明式 RenderGraph，自动推导图像屏障和信号量同步。**纯 pass 编排层，不依赖 scene/asset 等领域模块。**
- **`RenderGraphBuilder`**：构建 Pass 依赖图，声明资源读写关系。
- **`ComputePass`**：通用 compute pass 封装，接收 `FrameLabel` 和 `GlobalDescriptorSets` 作为参数（而非 `RenderContext`）。
- 支持 Timeline Semaphore 和 Binary Semaphore 的导入/导出。

### `truvis-renderer`
高层渲染管理器，负责组装所有子系统：
- **`Renderer`**：统一管理交换链（`RenderPresent`）、相机（`Camera`）等核心子系统，驱动每帧渲染循环。
- **`RenderContext`**：渲染期间不可变的全局状态聚合体，包含 `SceneManager`、`AssetHub`、`GpuScene`、`BindlessManager`、`GlobalDescriptorSets` 等。定义在此层（而非 render-graph 层），确保层次隔离。
- **`RenderPresent`**：交换链获取、呈现和重建（窗口 Resize）。

### `truvis-app`
应用框架层，面向应用开发者：
- **`OuterApp`** trait：定义 `init / update / draw / draw_ui / on_window_resized` 接口，开发者实现此 trait 即可构建渲染应用。
- **`GuiRgPass`**：ImGui 的 render graph 适配器，将 `GuiPass`（来自 gui-backend）包装为 `RgPass`。
- 内置 GUI 前端集成和平台抽象。
- 包含 `triangle`、`shader_toy`、`rt_cornell`、`rt_sponza` 等参考实现及 render pipeline。

### `truvis-scene`
场景数据管理：
- 几何体（`RtGeometry`、`TriangleSoA`）和 BLAS/TLAS 构建辅助。
- `SceneManager`：管理场景对象生命周期（基于 GUID）。

### `truvis-shader`
着色器编译工具链：
- 调用 Slang 编译器将 `.slang` 编译为 SPIR-V，输出到 `engine/shader/.build/`。
- `build.rs` 自动从 `.slangi` 头文件生成 Rust 类型（`truvis-shader-binding`）。

### `truvis-asset`
异步资产加载：
- **`AssetHub`**：统一资产注册表。
- **`AssetLoader`**：后台线程加载，返回 Handle，支持加载完成回调。
- **`AssetUploadManager`**：将 CPU 数据上传到 GPU（配合 staging buffer）。

### `truvis-gui-backend`
ImGui Vulkan 后端实现，负责字体纹理上传和 UI DrawData 的 GPU 渲染。**纯 Vulkan 录制层，不依赖 render-graph。** `GuiPass::draw` 接收显式参数（`FrameLabel`、`GlobalDescriptorSets`、`BindlessManager`）而非 `RenderContext`。Render graph 适配（`GuiRgPass`）由上层 `truvis-app` 负责。

---

## 数据流

### 核心类型流转

```
                    CPU 侧                              │            GPU 侧
                                                        │
 SceneManager                                           │
 ├── SlotMap<Mesh>          prepare_render_data()        │
 ├── SlotMap<Material>    ─────────────────────►  RenderData ──► GpuScene::upload()
 ├── SlotMap<Instance>      (查询 BindlessManager       │         ├── GfxStructuredBuffer<GpuInstance>
 └── SlotMap<PointLight>     和 AssetHub 解析纹理)       │         ├── GfxStructuredBuffer<GpuMaterial>
                                                        │         ├── GfxStructuredBuffer<GpuGeometry>
 AssetHub                                               │         ├── GfxStructuredBuffer<GpuPointLight>
 ├── AssetLoader (IO 线程)                               │         └── TLAS (加速结构)
 └── AssetUploadManager ──► staging buffer ─────────────►│──► GfxImage (纹理)
                                                        │
 BindlessManager                                        │
 └── 注册 SRV ──► bindless descriptor array ────────────►│──► Set 1 (shader 通过 index 访问)
```

### 关键数据类型

| 类型 | 定义位置 | 说明 |
|------|----------|------|
| `RenderData` | render-interface | 场景只读快照，CPU → GPU 桥梁 |
| `GpuScene` | render-interface | 管理 GPU 侧场景 buffer 和 TLAS |
| `GfxImageHandle` / `GfxBufferHandle` | render-interface | SlotMap 句柄，GPU 资源标识 |
| `BindlessSrvHandle` | render-interface | Bindless descriptor index，传入 shader |
| `AssetTextureHandle` | asset | 纹理资产句柄，跟踪 Loading→Uploading→Ready |
| `RgImageHandle` / `RgBufferHandle` | render-graph | 虚拟资源句柄，编译时映射到物理资源 |
| `RenderContext` | renderer | 只读聚合体，持有所有子系统引用 |

---

## 运行时序（每帧）

```
RenderApp::big_update()
│
├── 1. Renderer::begin_frame()
│      ├── 等待 GPU timeline semaphore（frame N-3 完成）
│      ├── GfxResourceManager::cleanup()  // 释放延迟销毁的资源
│      └── FrameCounter::advance()
│
├── 2. 输入处理 & 窗口 Resize
│      ├── InputManager::process_events()
│      └── CameraController::update()
│
├── 3. RenderPresent::acquire_next_image()
│
├── 4. GUI 帧构建
│      ├── GuiHost::begin_frame()       // imgui NewFrame
│      ├── OuterApp::draw_ui()          // 用户 UI 代码
│      └── GuiHost::end_frame()         // imgui Render
│
├── 5. 场景更新 & GPU 上传
│      ├── OuterApp::update()           // 用户更新场景
│      ├── AssetHub::update()           // 推进异步加载状态机
│      ├── SceneManager::prepare_render_data()  → RenderData
│      ├── GpuScene::upload_render_data()       // 写入 structured buffer + 构建 TLAS
│      └── GlobalDescriptorSets::update()       // 写入 per-frame UBO
│
├── 6. 渲染图构建与执行
│      ├── OuterApp::draw()             // 用户构建 RenderGraphBuilder
│      │     ├── add_pass(gbuffer / lighting / ...)
│      │     └── add_pass(gui_rg_pass)
│      ├── RenderGraphBuilder::compile()
│      │     ├── 拓扑排序
│      │     └── 自动插入 image barrier
│      └── CompiledGraph::execute()
│            └── 按拓扑序逐 pass 录制 GfxCommandBuffer
│
├── 7. 提交 & 呈现
│      ├── Queue::submit() with timeline semaphore signal
│      └── RenderPresent::present()
│
└── 8. Renderer::end_frame()
```

### 资产异步加载时序

```
帧 N:   AssetHub::load_texture("path")  → 返回 AssetTextureHandle (Loading)
         └── AssetLoader IO 线程开始读取文件
帧 N+k: AssetHub::update()
         ├── IO 完成 → 状态变为 Uploading
         └── AssetUploadManager 通过 staging buffer 上传 → 录制 copy cmd
帧 N+k+1: staging buffer copy 完成 → 状态变为 Ready
           └── BindlessManager 注册 SRV → 获得 BindlessSrvHandle
```

---

## 已知设计问题

1. **truvis-app 跨层依赖过多**：app 直接依赖 gfx / render-interface / render-graph / gui-backend 等底层模块，理想情况应由 renderer re-export，app 只依赖 renderer。根本原因是具体渲染管线（phong_pass、rt_pass 等）定义在 app 层。
2. **render-interface 职责过重**：同时承担接口定义（Handle 类型）和运行时管理（GpuScene、CmdAllocator、StageBufferManager）。`GpuScene` / `RenderData` 语义上属于 scene→GPU 桥梁，可考虑独立或下沉到 renderer。
3. **scene 层知道 bindless 细节**：`prepare_render_data()` 需要查询 `BindlessManager` 和 `AssetHub`，让 scene 层耦合了 GPU 绑定实现。更干净的做法是 scene 只输出逻辑句柄，由 renderer 负责解析。
4. **RenderContext 是 God Object**：聚合了所有子系统，任何 pass 通过 `RenderContext2` 都能访问一切，无法从类型层面约束 pass 的访问权限。

---

### `truvis-cxx`
C++ FFI 桥接层：
- 通过 `cxx-build` + CMake 集成 Assimp，提供场景文件（FBX / glTF）加载能力。
- `build.rs` 自动构建 C++ 库并将 DLL 复制到 `target/`。

### `truvis-utils`
通用工具库，当前提供 `NamedArray`（按名称索引的固定大小数组）及 `enumed_map!` / `count_indexed_array!` 等宏。
