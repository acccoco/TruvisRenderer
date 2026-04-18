# truvis-winit-app

`truvis-winit-app` 是平台入口层，负责窗口创建、事件循环与渲染线程启动。

## 主要职责

- 创建并管理 winit `EventLoop` 与窗口生命周期
- 将平台事件转换为引擎输入事件并转发
- 驱动渲染线程运行 `truvis-app::FrameRuntime` 与 `AppPlugin`

## 入口位置

- `src/bin/`：各示例应用可执行入口
- `src/app.rs`：平台运行时封装
- `src/render_loop.rs`：渲染线程主循环

## 启动方式

- 默认入口：`WinitApp::run_plugin(|| Box<dyn AppPlugin>)`
- 兼容入口：`WinitApp::run(|| Box<dyn OuterApp>)`（deprecated，仅兼容窗口保留）

## 模块边界

- 本模块不实现具体渲染算法，只负责平台与线程编排。
- 渲染细节在 `engine/crates/truvis-app` 与 `engine/crates/truvis-renderer`。
