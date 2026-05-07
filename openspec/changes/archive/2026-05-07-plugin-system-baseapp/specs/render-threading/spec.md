## MODIFIED Requirements

### Requirement: 渲染循环运行于独立线程

渲染循环 SHALL 继续运行于独立于 winit 主线程的 OS 线程。渲染线程内部 SHALL 持有 `Box<dyn FrameApp>`，并通过 `FrameApp` API 驱动 App；不得再创建或调用 `FrameRuntime`。

#### Scenario: App factory 创建 FrameApp

- **WHEN** `WinitApp::run_app`（或等价入口）被调用
- **THEN** winit 主线程创建窗口后 SHALL 将 raw handles 和 App factory 交给渲染线程
- **AND** 渲染线程 SHALL 创建 `Box<dyn FrameApp>` 并调用 `FrameApp::init_after_window`

#### Scenario: 渲染线程推进 FrameApp

- **WHEN** 渲染线程需要推进一帧
- **THEN** SHALL 调用 `FrameApp::time_to_render` 和 `FrameApp::run_frame`
- **AND** SHALL NOT 直接访问 RenderBackend、BaseApp 或 App 内部字段

### Requirement: 事件通过 crossbeam-channel 传递

事件仍 SHALL 通过 crossbeam-channel 从 winit 主线程传递到渲染线程。渲染线程 SHALL 将事件推入 `FrameApp::push_input_event`，由 App/BaseApp 在下一帧输入 hook 中处理。

#### Scenario: 输入事件交给 FrameApp

- **WHEN** 渲染线程从 channel drain 到 `InputEvent`
- **THEN** SHALL 调用 `app.push_input_event(event)`
- **AND** SHALL NOT 调用 FrameRuntime API

### Requirement: 二阶段关闭握手

关闭流程 SHALL 保持主线程与渲染线程二阶段握手。渲染线程观察到退出信号后 SHALL 调用 `FrameApp::shutdown(&mut self)`，由 App 关闭 Plugin 并销毁 BaseApp/RenderBackend/Gfx。

#### Scenario: App owns shutdown sequencing

- **WHEN** 渲染线程准备退出
- **THEN** SHALL 调用 `app.shutdown()`
- **AND** App SHALL 先 shutdown Plugin，再 destroy BaseApp
- **AND** 主线程仍 SHALL 等待渲染线程完成后再 drop Window
