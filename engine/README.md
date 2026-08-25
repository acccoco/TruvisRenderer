# Engine

`engine/` 是渲染引擎核心实现、Shader 工具链与 C++ FFI 边界目录。这里的 Rust crate 覆盖基础工具、Vulkan
RHI、CPU scene/assets、RenderGraph、RenderRuntime、App 框架和平台入口；具体 App、GUI 集成和业务 pass 位于
workspace 顶层 `app/`。

## 分层速览

真实依赖边界以 Cargo 依赖和 `../docs/summaries/` 中的当前事实为准，物理目录主要用于导航。总体方向是上层依赖下层，
同层 crate 默认不互相依赖，除非架构文档明确记录。

一级 Rust 职责目录使用 `eNN-` 前缀标识 Engine 归属和主要架构阶段；`e40-render/`、`e60-platform/` 等目录可以包含
多个实际 crate 层级。`shader/` 与 `cxx/` 保持稳定的横切工具链根目录。

- L0 基础层：`e00-foundation/`、`e00-utils/` 和 descriptor-layout crate，提供日志、路径、通用工具和 shader binding layout 元信息。
- L1 RHI 层：`e10-gfx/truvis-gfx` 封装 Vulkan root owner、typed Ctx、资源、队列、同步、swapchain 与管线基础能力。
- L2 渲染契约：`e40-render/truvis-render-foundation` 提供 `FrameLabel`、资源句柄、`RenderView` /
  `RenderSceneView` 和 `GfxResourceAccess`。
- L3 语义与编排辅助：`e30-world/` 保存 CPU asset/scene 语义，`e40-render/truvis-render-graph` 负责按 App 指定顺序推导 pass 同步。
- L4 Runtime 集成层：`e40-render/truvis-render-runtime` 持有 `Gfx`、`World`、GPU resource/binding/timing owners、runtime render state、`RenderWorld`、present、`RenderPassRecordCtx` 和 asset-to-GPU bridge。
- L5 App 框架层：`e50-app-frame/truvis-app-frame` 定义具体 `RenderApp` 阶段契约、统一 `RenderAppRunner` 和最小线程控制契约。
- L6 渲染线程宿主：`e60-platform/truvis-render-thread` 管理不依赖窗口 backend 的 OS RenderThread 生命周期。
- L7 窗口平台层：`e60-platform/truvis-winit-host` 管理 winit standalone / embedded 窗口与输入适配。
- L8 具体应用层：主体应用、Editor 和 samples 位于 `../app/`。

`shader/` 和 `cxx/` 是工具链与外部边界目录：其中 binding crate 会被运行时 crate 使用，build crate 主要由 `just`
命令驱动生成产物。

## 目录与 crate

### `e00-foundation/`

基础工具层，不依赖窗口、渲染运行时或 App 业务语义。

- `truvis-utils/`：通用小工具 crate，提供带索引常量数组、基础配置解析等 helper；不承载 Vulkan、asset 或 App
  生命周期语义。
- `truvis-logs/`：项目统一日志初始化和 formatter；业务 crate 继续使用 `log` facade，不在调用点手工拼接线程上下文。

### `e00-utils/`

引擎工具层，面向 workspace 路径和资源准备，不等同于运行时 asset 系统。

- `truvis-path/`：基于根目录 `map.toml` 的统一路径入口，提供 workspace、assets、resources、shader build、CXX、
  运行时路径编码和词法路径归一化等 helper；不负责下载或加载资源内容。
- `truvis-fetch-res/`：`fetch_res` 工具 crate，读取 `resources.toml` 并下载模型资产、外部工具、SDK 或参考源码资源；不参与渲染线程的
  asset loading。

### `e10-gfx/`

Vulkan RHI 与 descriptor-layout 辅助层，提供底层 GPU 能力，不包含 scene、App 或具体 pass 编排。

- `truvis-gfx/`：`Gfx` root owner、typed Gfx Ctx、Vulkan device/queue/resource/sync/swapchain/pipeline
  wrapper；不依赖更高层渲染或业务语义。
- `truvis-descriptor-layout-trait/`：描述 shader descriptor binding layout 的 trait 和 binding item 数据结构；不做宏解析或
  descriptor 分配。
- `truvis-descriptor-layout-macro/`：`DescriptorBinding` derive 宏，按字段属性生成 binding layout 元信息；不持有运行时 GPU
  资源。

### `e30-world/`

CPU 侧语义层，负责 asset 身份、加载状态、scene runtime 身份与 `World` 聚合，不创建 GPU 资源。

- `truvis-asset/`：纹理、mesh、material、model 等内容资产的 CPU 身份、去重、加载状态和完成事件；不创建 GPU
  image/buffer、BLAS、bindless index 或 material slot。
- `truvis-world/`：`World`、`SceneStore` 和 `AssetHub` 聚合入口；不持有 `Gfx`、GPU resource/binding owner、swapchain 或 frame state。

### `e40-render/`

通用渲染基础设施目录，覆盖 GPU 状态、RenderGraph 同步辅助和 Runtime 集成。

- `truvis-render-foundation/`：跨渲染 crate 的最小契约层，提供 `FrameLabel`、GPU 资源句柄、
  `RenderView`、`RenderSceneView` 和 `GfxResourceAccess`；不包含 GPU owner、CPU scene、窗口平台或 runtime render state 语义。
- `truvis-render-graph/`：按 App 添加 pass 的线性顺序推导 image barrier、layout transition 和 semaphore submit
  信息；不做自动调度、资源 aliasing 或业务 pass 逻辑。
- `truvis-render-runtime/`：渲染运行时集成层，拥有 `Gfx`、`World`、`GfxResourceManager`、`ShaderBindingSystem`、`CmdAllocator`、`PerFrameGpuData`、runtime render state、runtime 私有 `RenderWorld`、present、同步资源、`RenderPassRecordCtx` 和 CPU-to-GPU bridge；不负责窗口事件循环、GUI 适配或具体 App pass 顺序。

### `e50-app-frame/`

平台无关的 App 框架目录，定义应用、固定帧执行器和 Runner 消费的跨线程契约。

- `truvis-app-frame/`：定义具体 `RenderApp`、phase Ctx、统一 `RenderAppRunner` 和最小线程控制契约；不依赖
  `winit`，也不持有具体 App/子系统业务状态。

### `e60-platform/`

窗口和线程宿主目录，保持窗口 backend 与 OS 渲染线程生命周期独立。

- `truvis-render-thread/`：backend-independent OS RenderThread owner，负责线程启动、App factory、完成/panic 握手与
  join；只依赖 frame 契约，不依赖 winit、Tauri 或 Windows API。
- `truvis-winit-host/`：winit standalone / embedded 窗口 owner，负责 EventLoop、输入适配、Win32 typed handle 交接和
  child HWND；窗口标题、图标资源与日志初始化由具体 app 入口决定。

### `shader/`

Shader 源码、SPIR-V 编译和 Rust 绑定生成目录。

- `entry/`、`api/`、`lib/`：Slang shader 入口、共享 ABI/API 头文件和 shader 侧复用库。共享结构变更会影响 Rust 绑定。
- `truvis-shader-build/`：`shader-build` 工具 crate，负责把 shader 源码编译到 `build/shader/`；推荐通过 `just shader` 调用。
- `truvis-shader-binding/`：通过 bindgen 生成 Rust 侧 GPU 数据结构绑定，并导出 `truvis_shader_binding::gpu`；不负责 shader
  编译或 pass 录制。

### `cxx/`

C++ 子系统、CMake/vcpkg 构建和 Rust FFI binding 目录。

- `mods/`：C++ 模块源码，当前包含 Assimp、Streamline、GFX 和公共 utils 等 native 模块；导出到 Rust 的能力通过对应模块的 C API
  暴露。
- `truvis-cxx-build/`：CXX 构建驱动 crate，选择 CMake preset、构建 native 产物、复制 `.lib`/`.dll`/`.pdb` 并同步
  `compile_commands.json`；推荐通过 `just cxx` 调用。
- `truvis-assimp-binding/`：Assimp C API 的 Rust FFI 声明 crate；只负责绑定和链接声明，不负责 asset 调度、CPU 数据状态机或
  GPU 上传。
- `truvis-streamline-binding/`：Streamline / DLSS Rust 绑定与最小 RAII runtime，负责 `slInit`/`slShutdown` 生命周期和日志桥；当前不负责
  RenderGraph pass、resource tagging 或 DLSS evaluate。

应用层位于 workspace 顶层 `../app/`：`app-kit/` 保存生命周期契约、相机/输入和纯 CPU 状态；`app-imgui/`
拥有 ImGui subsystem 与私有 backend；`app-render-passes/` 保存共享 GPU pass；`app-rendering/` 拥有 realtime/offline
渲染子系统和长期资源；`app-render-ui/` 集成渲染设置与 ImGui。`truvis/` 与 `samples/` 保存可执行入口。

## 推荐阅读顺序

1. `../docs/ARCHITECTURE.md`：先确认当前架构入口、阅读顺序与最高优先级约束。
2. `../docs/summaries/`：按主题阅读分层依赖、帧生命周期、Runtime/App/Subsystem 边界、RenderGraph 数据流、线程与资源生命周期。
3. 本文件：按目录和 crate 定位要阅读的模块。
4. 各 crate 内 README：深入具体职责、生命周期和边界；重点可先看 `e10-gfx/truvis-gfx/README.md`、`e30-world/truvis-asset/README.md`、
   `e30-world/truvis-world/README.md`、`e40-render/*/README.md`、`e50-app-frame/*/README.md`、`e60-platform/*/README.md`。
5. `shader/README.md`、`cxx/README.md`：了解 shader/CXX 工具链与外部边界。

## 构建与工具入口

运行渲染示例前优先参考根目录 `justfile`：

- `just fetch-res`：下载 `resources.toml` 声明的资源与工具。
- `just shader`：编译 shader 并更新 `truvis-shader-binding`。
- `just cxx`：构建 C++ native 产物并更新 Assimp / Streamline Rust binding。
- `just build-all`：依次准备 shader、CXX，再构建整个 workspace。
