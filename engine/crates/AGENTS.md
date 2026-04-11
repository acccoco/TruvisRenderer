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

### `truvis-cxx`
C++ FFI 桥接层：
- 通过 `cxx-build` + CMake 集成 Assimp，提供场景文件（FBX / glTF）加载能力。
- `build.rs` 自动构建 C++ 库并将 DLL 复制到 `target/`。

### `truvis-utils`
通用工具库，当前提供 `NamedArray`（按名称索引的固定大小数组）及 `enumed_map!` / `count_indexed_array!` 等宏。
