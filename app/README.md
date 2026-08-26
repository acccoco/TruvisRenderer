# app

`app/` 是项目的应用域目录，放置 app 层公共组件、主体 Truvis app 和独立 samples。这里的 crate 依赖引擎层能力，但不向 `engine/` 反向注入业务代码。

## 目录职责

- [`app-kit/`](app-kit/README.md)：生命周期契约、输入/相机控制和不依赖具体渲染子系统的纯 CPU 状态。
- [`app-imgui/`](app-imgui/README.md)：ImGui 子系统、私有 Vulkan backend 和通用诊断控件。
- [`app-render-passes/`](app-render-passes/README.md)：ray tracing、post-process 和产品效果的共享 GPU pass。
- [`app-rendering/`](app-rendering/README.md)：realtime/offline 渲染子系统、配置和长期 GPU 资源。
- [`app-render-ui/`](app-render-ui/README.md)：渲染设置与 ImGui 的共享集成，既提供 realtime-only 窗口也提供可组合 section。
- `editor/`：Web editor 子系统；`bridge` 定义协议与跨线程 endpoint，`server` 提供本地 HTTP / WebSocket，`web`
  是 React / TypeScript 页面。场景协议到 `World` 的适配保留在 `truvis/src/editor_controller.rs`。
- [`truvis/`](truvis/README.md)：主体 Tauri app crate，组合 WebView、embedded viewport、Editor controller、
  selection/overlay 和 realtime/offline 渲染子系统，并显式决定 Renderer 业务顺序。
- `samples/hello-triangle/`：Triangle 示例，提供 `triangle`。
- `samples/shader-toy/`：ShaderToy 示例，提供 `shader-toy`。
- `samples/cornell/`：Cornell Box 光追示例，提供 `rt-cornell`。

## 边界约束

- `app-kit` 不依赖 ImGui、GPU pass、App shader binding 或具体渲染子系统；相机、输入与生命周期契约可被所有 Renderer 直接复用。
- `app-imgui` 与 `app-rendering` 分别依赖 `app-kit`；渲染子系统不依赖 ImGui。
- `app-rendering -> app-render-passes`；`app-render-ui -> app-imgui + app-rendering + app-kit`，不直接依赖 pass crate。
- Triangle / ShaderToy 只组合 `app-kit + app-imgui`，sample 专属 pass 留在对应 crate；Cornell 再组合 rendering/UI，主体 Truvis 可直接使用共享产品效果 pass。
- sample 入口自行初始化日志并提供窗口标题、尺寸、透明策略和图标内容；平台宿主不决定产品外观。
- 主体 app 的顶层窗口和 WebView 由 Tauri/Tao main thread 持有，child render 窗口和独立 samples 的顶层窗口由
  `engine/e60-platform/truvis-winit-host` 提供；`engine/e60-platform/truvis-render-thread` 在 OS 渲染线程内执行具体
  `Box<dyn Renderer>` factory，并交给唯一的 `RenderLoop`。
