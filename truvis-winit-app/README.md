# truvis-winit-app

`truvis-winit-app` 是平台入口层，负责窗口创建、事件循环与渲染线程启动。

## 主要职责

- 创建并管理 winit `EventLoop` 与窗口生命周期
- 将平台事件转换为引擎输入事件并转发
- 驱动渲染线程运行 `Box<dyn RenderApp>`，示例入口通常传入 `RenderAppShell<DemoState>`

## 入口位置

- `src/bin/`：各示例应用可执行入口
- `src/app.rs`：平台运行时封装
- `src/render_loop.rs`：渲染线程主循环

## 启动方式

- 入口：`WinitApp::run_app(|| Box<dyn RenderApp>)`
- 示例：`WinitApp::run_app(|| Box::new(RenderAppShell::new(DemoState::default())))`

## 模块边界

- 本模块不实现具体渲染算法，只负责平台与线程编排。
- App / Plugin 契约定义在 `engine/crates/truvis-frame-api`，帧骨架在 `engine/crates/truvis-frame-runtime`。
- 渲染后端在 `engine/crates/truvis-render-backend`，通用 pass 在 `engine/crates/truvis-render-passes`。
