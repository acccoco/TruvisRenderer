# truvis-winit-app

`truvis-winit-app` 是平台入口层，负责窗口创建、事件循环与渲染线程启动。

## 主要职责

- 创建并管理 winit `EventLoop` 与窗口生命周期
- 将平台事件转换为引擎输入事件并转发
- 驱动渲染线程运行 `Box<dyn RenderApp>`，示例入口通常传入 `RenderAppShell<DemoState>`

## 入口位置

- `src/bin/`：各示例应用可执行入口
- `src/app.rs`：平台运行时封装
- `src/winit_event_adapter.rs`：winit 事件到 `InputEvent` 的转换

## 启动方式

- 入口：`WinitApp::run_app(|| Box<dyn RenderApp>)`
- 示例：`WinitApp::run_app(|| Box::new(RenderAppShell::new(DemoState::default())))`

## 线程模型

- main thread 持有 winit `EventLoop` 和 `Window`，负责接收平台事件。
- render thread 持有 `Box<dyn RenderApp>`，所有 Vulkan 对象都在该线程创建、使用和销毁。
- 输入事件通过 channel 传给 render thread，再由 `RenderApp::push_input_event` 进入 runtime shell 的输入队列。
- resize 使用 latest-size 模式合并连续事件；零尺寸窗口不会触发 swapchain 重建。
- 退出时 main thread 发出退出信号，render thread 完成 `RenderApp::shutdown` 和 GPU 资源释放后，main thread 再 join 渲染线程并允许 `Window` drop。

## 模块边界

- 本模块不实现具体渲染算法，只负责平台与线程编排。
- App / Plugin 契约、帧骨架与 render loop 定义在 `engine/frame/truvis-app-frame`。
- 渲染运行时在 `engine/render/truvis-render-runtime`，通用 pass 在 `engine/render/truvis-render-passes`。
