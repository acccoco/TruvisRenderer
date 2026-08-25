# app

`app/` 是项目的应用域目录，放置 app 层公共组件、主体 Truvis app 和独立 samples。这里的 crate 依赖引擎层能力，但不向 `engine/` 反向注入业务代码。

## 目录职责

- `app-kit/`：公共 app 组件，包括 GUI、输入/相机控制、overlay 与 RT pipeline glue。
- `editor/`：Web editor 子系统；`bridge` 定义协议与跨线程 endpoint，`server` 提供本地 HTTP / WebSocket，`web`
  是 React / TypeScript 页面。场景协议到 `World` 的适配保留在 `truvis/src/editor_controller.rs`。
- [`truvis/`](truvis/README.md)：主体 Tauri app crate，组合 WebView、embedded viewport、Editor controller、
  selection/overlay 和 realtime/offline pipeline，并显式决定 App 业务顺序。
- `samples/hello-triangle/`：Triangle 示例，提供 `triangle`。
- `samples/shader-toy/`：ShaderToy 示例，提供 `shader-toy`。
- `samples/cornell/`：Cornell Box 光追示例，提供 `rt-cornell`。

## 边界约束

- `app-kit` 只放可复用组件，不放具体 app state。
- sample 专用 pass 留在对应 sample crate 内。
- 主体 app 的顶层窗口和 WebView 由 Tauri/Tao main thread 持有，child render 窗口和独立 samples 的顶层窗口由
  `engine/app-frame/truvis-winit-app` 提供；app crate 只向渲染入口注入具体 `Box<dyn RenderApp>`，平台 `RenderThread`
  在 OS 渲染线程内执行 factory，并交给唯一的 `RenderAppRunner`。
