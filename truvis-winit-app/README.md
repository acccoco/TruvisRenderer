# truvis-winit-app

`truvis-winit-app` 是平台入口层，负责窗口创建、事件循环与渲染线程启动。

## 主要职责

- 创建并管理 winit `EventLoop` 与窗口生命周期
- 将平台事件转换为引擎输入事件并转发
- 驱动渲染线程运行 `truvis-app` 提供的应用逻辑

## 入口位置

- `src/bin/`：各示例应用可执行入口
- `src/app.rs`：平台运行时封装

## 模块边界

- 本模块不实现具体渲染算法，只负责平台与线程编排。
- 渲染细节在 `engine/crates/truvis-app` 与 `engine/crates/truvis-renderer`。
