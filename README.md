# TruvisRenderer

[![CI Status](https://github.com/acccoco/TruvisRenderer/workflows/Rust/badge.svg)](https://github.com/acccoco/TruvisRenderer/actions)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/acccoco/TruvisRenderer)

基于 Rust + Vulkan 1.3+ 的实时渲染引擎，覆盖从 RHI、RenderGraph 到光线追踪应用层的完整链路。

## 项目亮点

- 光线追踪：支持 Cornell Box 与 Sponza 等典型场景
- RenderGraph：按 pass 添加顺序录制，并基于 image 声明自动同步
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
just triangle
just cornell
just sponza
just shader-toy
```

使用 Vulkan validation layer 运行光追示例：

```powershell
just cornell-validation
just sponza-validation
```

## 运行时架构（当前）

- 平台入口：`engine/frame/truvis-winit-app` 通过 `WinitApp::run_app(...)` 启动渲染线程
- App 公共组件：`truvis-app/app-kit` 提供 GUI、输入/相机、overlay 与 RT pipeline 等 app 层复用能力
- 主体 App：`truvis-app/sponza` 提供 `rt-sponza`，samples 位于 `truvis-app/samples/*`
- App 适配：app state 实现 `RenderAppHooks`，由 `truvis-app-frame::RenderAppShell` 包装成 render loop 需要的 `RenderApp`
- 帧骨架：`truvis-app-frame::RenderAppShell` 持有 `RenderRuntime` 与输入事件队列，负责 `input -> update -> plugin update -> prepare -> render -> present` 固定顺序
- Plugin 组合：app state 通过 `RenderAppHooks::visit_plugins_mut` 声明标准生命周期 Plugin 顺序；GUI 与渲染管线的特有能力通过具体类型方法暴露
- 渲染运行时：`truvis-render-runtime::RenderRuntime` 聚焦帧生命周期、CPU/GPU 同步与 GPU 数据上传
- swapchain 重建：渲染线程通过 `RenderApp::recreate_swapchain_if_needed` 触发，`RenderAppShell` 在实际重建后通知 app state 并批量调用 Plugin resize

## 文档导航

- 架构总览：[`ARCHITECTURE.md`](./ARCHITECTURE.md)
- AI 协作规则：[`AGENTS.md`](./AGENTS.md)
- 模块说明：各关键目录下 `README.md`（如 `engine/`、`engine/shader/`、`truvis-app/`）

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
