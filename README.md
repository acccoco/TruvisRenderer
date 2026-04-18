# TruvisRenderer

[![CI Status](https://github.com/acccoco/TruvisRenderer/workflows/Rust/badge.svg)](https://github.com/acccoco/TruvisRenderer/actions)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/acccoco/TruvisRenderer)

基于 Rust + Vulkan 1.3+ 的实时渲染引擎，覆盖从 RHI、RenderGraph 到光线追踪应用层的完整链路。

## 项目亮点

- 光线追踪：支持 Cornell Box 与 Sponza 等典型场景
- RenderGraph：声明式资源依赖与自动同步管理
- Slang 工具链：自动编译 shader 并生成 Rust 绑定
- Bindless 渲染：统一资源访问模型，降低绑定切换开销
- ImGui 集成：便于实时调试和参数调整
- C++ FFI 资产加载：通过 Assimp 支持 FBX / glTF / OBJ 等格式

![Result](./docs/imgs/Result.PNG)
![rt-sponza](./docs/imgs/rt-sponza.png)

## 快速开始

### 环境要求

- Rust 1.75+
- Vulkan SDK 1.3+
- CMake 3.20+
- Visual Studio 2019+（Windows）

### 构建步骤

```powershell
# 1) 拉取资源与工具
cargo run --bin fetch_res

# 2) 构建 C++ 模块
cargo run --bin cxx-build

# 3) 编译 Shader（运行渲染程序前必须执行）
cargo run --bin shader-build

# 4) 构建 Workspace
cargo build --all
```

### 运行示例

```powershell
cargo run --bin triangle
cargo run --bin rt-cornell
cargo run --bin rt-sponza
cargo run --bin shader-toy
```

## 运行时架构（当前）

- 平台入口：`truvis-winit-app` 通过 `WinitApp::run_plugin(...)` 启动渲染线程
- 帧编排：`truvis-app::FrameRuntime` 负责 phase 调度（`input -> build_ui -> update -> prepare -> render -> present`）
- 应用扩展：demo 通过 `AppPlugin` 接入；旧 `OuterApp` 仅保留兼容路径（deprecated）
- 渲染后端：`truvis-renderer::Renderer` 聚焦 backend 执行与 GPU 数据上传
- swapchain 重建：统一由 runtime 单入口处理，覆盖窗口尺寸变化与 backend `need_resize`（out-of-date/suboptimal）

## 文档导航

- 架构总览：[`ARCHITECTURE.md`](./ARCHITECTURE.md)
- AI 协作规则：[`AGENTS.md`](./AGENTS.md)
- 模块说明：各关键目录下 `README.md`（如 `engine/`、`engine/crates/`、`engine/shader/`）

## 展示特性

### Irradiance Cache

使用 HashGrid 缓存 diffuse 表面的光照信息，提升全局光照阶段的复用效率。

![IrradianceCache](./docs/imgs/IrradianceCache.png)

### 剖切与填充面

支持剖切体与填充面的效果渲染。

![Section](./docs/imgs/Result-Section.PNG)
![Section-Fill](./docs/imgs/Section-Fill-Result.PNG)

### SER（Shader Execution Reordering）

在光追路径中按材质类型重排执行，提高线程一致性与缓存命中率。

![SER compare](./docs/imgs/SER-compare.png)
