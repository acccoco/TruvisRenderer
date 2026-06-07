# Engine

`engine/` 是渲染引擎核心实现、Shader 工具链与 C++ FFI 边界目录。这里的 Rust crate 覆盖基础工具、Vulkan
RHI、CPU scene/assets、RenderGraph、RenderRuntime、App 框架和平台入口；具体 App、GUI 集成和业务 pass 位于
workspace 顶层 `app/`。

## 分层速览

真实依赖边界以 Cargo 依赖和 `../docs/summaries/` 中的当前事实为准，物理目录主要用于导航。总体方向是上层依赖下层，
同层 crate 默认不互相依赖，除非架构文档明确记录。

- L0 基础层：`foundation/`、`utils/` 和 descriptor-layout crate，提供日志、路径、通用工具和 shader binding layout 元信息。
- L1 RHI 层：`gfx/truvis-gfx` 封装 Vulkan root owner、typed Ctx、资源、队列、同步、swapchain 与管线基础能力。
- L2 GPU 基础设施：`render/truvis-render-foundation` 提供 `GpuStore`、全局 descriptor、bindless、资源句柄和 frame state。
- L3 语义与编排辅助：`world/` 保存 CPU asset/scene 语义，`render/truvis-render-graph` 负责按 App 指定顺序推导 pass 同步。
- L4 Runtime 集成层：`render/truvis-render-runtime` 持有 `Gfx`、`World`、`GpuStore`、GPU scene、present 和 asset-to-GPU
  bridge。
- L5 App 框架层：`app-frame/truvis-app-frame` 定义 `RenderApp`、`RenderAppShell`、`RenderAppHooks`、`Plugin` 和 render loop
  契约。
- L6 平台与应用层：`app-frame/truvis-winit-app` 负责 winit 平台入口；具体应用和 samples 位于 `../app/`。

`shader/` 和 `cxx/` 是工具链与外部边界目录：其中 binding crate 会被运行时 crate 使用，build crate 主要由 `just`
命令驱动生成产物。

## 目录与 crate

### `foundation/`

基础工具层，不依赖窗口、渲染运行时或 App 业务语义。

- `truvis-utils/`：通用小工具 crate，目前提供带索引常量数组等 helper；不承载 Vulkan、asset 或 App 生命周期语义。
- `truvis-logs/`：项目统一日志初始化和 formatter；业务 crate 继续使用 `log` facade，不在调用点手工拼接线程上下文。

### `utils/`

引擎工具层，面向 workspace 路径和资源准备，不等同于运行时 asset 系统。

- `truvis-path/`：基于根目录 `map.toml` 的统一路径入口，提供 workspace、assets、resources、shader build、CXX 等路径
  helper；不负责下载或加载资源内容。
- `truvis-fetch-res/`：`fetch_res` 工具 crate，读取 `resources.toml` 并下载模型资产或外部工具资源；不参与渲染线程的 asset
  loading。

### `gfx/`

Vulkan RHI 与 descriptor-layout 辅助层，提供底层 GPU 能力，不包含 scene、App 或具体 pass 编排。

- `truvis-gfx/`：`Gfx` root owner、typed Gfx Ctx、Vulkan device/queue/resource/sync/swapchain/pipeline
  wrapper；不依赖更高层渲染或业务语义。
- `truvis-descriptor-layout-trait/`：描述 shader descriptor binding layout 的 trait 和 binding item 数据结构；不做宏解析或
  descriptor 分配。
- `truvis-descriptor-layout-macro/`：`DescriptorBinding` derive 宏，按字段属性生成 binding layout 元信息；不持有运行时 GPU
  资源。

### `world/`

CPU 侧语义层，负责 asset 身份、加载状态、scene runtime 身份与 `World` 聚合，不创建 GPU 资源。

- `truvis-asset/`：纹理、mesh、material、model 等内容资产的 CPU 身份、去重、加载状态和完成事件；不创建 GPU
  image/buffer、BLAS、bindless index 或 material slot。
- `truvis-world/`：`World`、`SceneManager` 和 `AssetHub` 聚合入口；不持有 `Gfx`、`GpuStore`、swapchain 或 frame state。

### `render/`

通用渲染基础设施目录，覆盖 GPU 状态、RenderGraph 同步辅助和 Runtime 集成。

- `truvis-render-foundation/`：`GpuStore`、`GfxResourceManager`、`BindlessManager`、`GlobalDescriptorSets`、`RenderSceneView`
  、frame/cmd 资源等 GPU 基础状态；不包含 CPU scene 或窗口平台语义。
- `truvis-render-graph/`：按 App 添加 pass 的线性顺序推导 image barrier、layout transition 和 semaphore submit
  信息；不做自动调度、资源 aliasing 或业务 pass 逻辑。
- `truvis-render-runtime/`：渲染运行时集成层，拥有 `Gfx`、`World`、`GpuStore`、runtime 私有 `GpuScene`、present、同步资源和
  CPU-to-GPU bridge；不负责窗口事件循环、GUI 适配或具体 App pass 顺序。

### `app-frame/`

App 框架和平台入口目录，把平台无关的 App 契约与 winit 平台启动分开。

- `truvis-app-frame/`：定义 `RenderApp`、`RenderAppHooks`、`Plugin`、phase Ctx、`RenderAppShell` 和 render loop 共享状态；不依赖
  `winit`，也不持有具体 App/Plugin 业务状态。
- `truvis-winit-app/`：winit 平台入口，负责窗口、事件循环、输入事件适配和渲染线程启动；通过 `Box<dyn RenderApp>` 注入具体
  App，不依赖主体 app 或 samples。

### `shader/`

Shader 源码、SPIR-V 编译和 Rust 绑定生成目录。

- `entry/`、`share/`、`lib/`：Slang shader 入口、共享头文件和 shader 侧复用库。共享结构变更会影响 Rust 绑定。
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

应用层位于 workspace 顶层 `../app/`，其中 `app-kit/` 保存 GUI、私有 GUI backend、输入/相机、overlay 和 RT pipeline glue，
`app-render-passes/` 保存主体 app 与 samples 共享的具体 pass，`truvis/` 与 `samples/` 保存可执行入口。

## 推荐阅读顺序

1. `../docs/ARCHITECTURE.md`：先确认当前架构入口、阅读顺序与最高优先级约束。
2. `../docs/summaries/`：按主题阅读分层依赖、帧生命周期、Runtime/App/Plugin 边界、RenderGraph 数据流、线程与资源生命周期。
3. 本文件：按目录和 crate 定位要阅读的模块。
4. 各 crate 内 README：深入具体职责、生命周期和边界；重点可先看 `gfx/truvis-gfx/README.md`、`world/truvis-asset/README.md`、
   `world/truvis-world/README.md`、`render/*/README.md`、`app-frame/*/README.md`。
5. `shader/README.md`、`cxx/README.md`：了解 shader/CXX 工具链与外部边界。

## 构建与工具入口

运行渲染示例前优先参考根目录 `justfile`：

- `just fetch-res`：下载 `resources.toml` 声明的资源与工具。
- `just shader`：编译 shader 并更新 `truvis-shader-binding`。
- `just cxx`：构建 C++ native 产物并更新 Assimp / Streamline Rust binding。
- `just build-all`：依次准备 shader、CXX，再构建整个 workspace。
