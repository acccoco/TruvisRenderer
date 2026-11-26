# Engine

`engine/` 是渲染引擎核心实现、运行时基础设施和 Shader 工具链目录。Rust crate 覆盖
基础设施、Vulkan RHI、CPU scene/assets、RenderGraph、RenderRuntime、RenderLoop 和平台入口；
具体 Renderer、GUI 集成和业务 pass 位于 workspace 顶层 `renderer/`，应用启动壳位于 `app/`。

## 目录职责

物理目录用于导航，实际依赖边界以 Cargo manifest、`docs/ARCHITECTURE.md` 和 `docs/design/` 为准。
运行时目录内部仍使用 `eNN-` 前缀表示 Engine 的架构阶段；工具目录不属于运行时层。

- `tools/`：资源准备、shader 编译、shader binding codegen 和 CXX binding 构建支持。
- `runtime/e00-foundation/`：运行时和构建工具共用的日志、通用工具、工作区路径与 Shader 清单。
- `runtime/e10-gfx/`：Vulkan RHI、资源、队列、同步、swapchain、pipeline 和 descriptor layout。
- `runtime/e30-world/`：CPU asset、scene、World 和领域数据验证。
- `runtime/e40-render/`：Render foundation、RenderGraph、RenderRuntime、RenderWorld 和 GPU bridge。
- `runtime/e50-render-loop/`：Renderer phase contract、RenderLoop 和线程控制契约。
- `runtime/e60-platform/`：RenderThread、standalone/embedded winit host 和窗口输入适配。
- `shader/`：Engine-owned shader 源码、ABI 和 `truvis-shader-binding`。

Renderer 层位于 workspace 顶层 `../renderer/`，应用层位于 `../app/`。Engine 不依赖这两个顶层业务目录。

## 工具与运行时边界

`engine/tools/` 中的 crate 只负责构建期或资源准备工作，不拥有 Vulkan、RenderGraph、GPU resource
或线程生命周期。它们可以依赖 `runtime/e00-foundation/` 中的路径和 shader manifest crate；目录分类不
替代 Cargo 实际依赖方向，也不引入兼容转发层。

`engine/shader/` 是 Shader ABI owner。绑定 crate 从 owner 的 ABI 投影生成 Rust 类型，不负责 shader
编译或 pass 录制。

workspace 根 `cxx/` 仍是唯一 native integration project：native module 和 public C ABI 属于
`cxx/modules`，Rust binding 位于 `cxx/bindings`，`engine/tools/truvis-cxx-binding-build` 只提供
binding 生成、链接和显式运行文件复制支持。

## 推荐阅读和入口

1. `../docs/ARCHITECTURE.md`：架构入口、依赖方向和全局约束。
2. `../docs/design/`：Runtime、RenderGraph、线程、资源生命周期和 shader/native 边界。
3. `runtime/*/README.md`：具体运行时 crate 职责和生命周期。
4. [工具](tools/README.md)和[基础设施](runtime/e00-foundation/README.md)的模块入口。
5. `shader/README.md` 与 `../cxx/README.md`：shader 工具链和 native integration。

## 构建入口

运行渲染示例前优先参考根目录 `justfile`：

- `just fetch-res`：下载 `resources.toml` 声明的资源与工具。
- `just shader`：编译 shader 并更新 shader binding。
- `just cxx`：构建 C++ native 产物并更新 Assimp / Streamline Rust binding。
- `just build-all`：依次准备 shader、CXX，再构建整个 workspace。
