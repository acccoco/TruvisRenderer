# Engine Crates

渲染引擎各功能模块，按依赖层次从底层到上层排列。

## 依赖层次

```
truvis-gfx
  └── truvis-render-interface
        └── truvis-render-graph
              └── truvis-renderer
                    └── truvis-app
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
声明式 RenderGraph，自动推导图像屏障和信号量同步：
- **`RenderGraphBuilder`**：构建 Pass 依赖图，声明资源读写关系。
- **`RenderContext`**：编译后的图，负责在 CommandBuffer 上执行 Pass 并插入正确的 `vkCmdPipelineBarrier2`。
- 支持 Timeline Semaphore 和 Binary Semaphore 的导入/导出。

### `truvis-renderer`
高层渲染管理器：
- **`Renderer`**：统一管理交换链（`RenderPresent`）、`RenderContext`、相机（`Camera`）等核心子系统，驱动每帧渲染循环。
- **`RenderPresent`**：交换链获取、呈现和重建（窗口 Resize）。

### `truvis-app`
应用框架层，面向应用开发者：
- **`OuterApp`** trait：定义 `init / update / draw / draw_ui / on_window_resized` 接口，开发者实现此 trait 即可构建渲染应用。
- 内置 GUI 前端集成（ImGui）和平台抽象。
- 包含 `triangle`、`shader_toy`、`rt_cornell`、`rt_sponza` 等参考实现。

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
ImGui Vulkan 后端实现，负责字体纹理上传和 UI DrawData 的 GPU 渲染。

### `truvis-cxx`
C++ FFI 桥接层：
- 通过 `cxx-build` + CMake 集成 Assimp，提供场景文件（FBX / glTF）加载能力。
- `build.rs` 自动构建 C++ 库并将 DLL 复制到 `target/`。

### `truvis-utils`
通用工具库，当前提供 `NamedArray`（按名称索引的固定大小数组）。
