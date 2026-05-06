## MODIFIED Requirements

### Requirement: 渲染循环运行于独立线程

渲染循环 SHALL 在独立于 winit 主线程的 OS 线程中执行。winit 主线程仅负责 window 生命周期与事件 pump，不得直接调用 `RenderApp::run_frame` 或任何 Vulkan API。

#### Scenario: 进程启动

- **WHEN** `WinitApp::run_app` 被调用
- **THEN** 主线程创建 winit `EventLoop`
- **AND** 在 `resumed` 回调中创建 `Window` 后，spawn 一条渲染线程并传递 window handles
- **AND** 渲染线程内部创建 `Box<dyn RenderApp>`，通常由 `RenderAppShell::new(app_hooks)` 产生
- **AND** 渲染线程内部完成 `Gfx::init` / `RenderBackend::new` / `init_after_window`
- **AND** 主线程后续仅执行事件 pump 与退出握手

#### Scenario: 每帧渲染由渲染线程自主驱动

- **WHEN** 渲染线程处于主循环中
- **THEN** 渲染线程 SHALL 自行决定何时推进帧（通过 `RenderApp::time_to_render()` 或等价机制）
- **AND** 主线程 SHALL NOT 依赖 winit 的 `RedrawRequested` 事件驱动 Vulkan 渲染
- **AND** 主线程 SHALL NOT 再调用 `Window::request_redraw`

### Requirement: 事件通过 crossbeam-channel 传递

主线程 SHALL 把 winit `WindowEvent` 翻译为 `InputEvent` 后，通过 `crossbeam-channel` 的 unbounded 通道投递给渲染线程。渲染线程 SHALL 将事件推入 `RenderApp::push_input_event`，由 `RenderAppShell` 在下一帧 input 阶段交给 `RenderAppHooks::on_input`。

#### Scenario: 普通输入事件

- **WHEN** winit 在主线程上发出 `KeyboardInput` / `MouseInput` / `MouseMotion` 等事件
- **THEN** 主线程 SHALL 翻译为 `InputEvent` 并非阻塞地 `send` 到事件通道
- **AND** 渲染线程 SHALL 在每轮主循环开头一次性 drain 通道中所有事件
- **AND** 渲染线程 SHALL 调用 `RenderApp::push_input_event` 灌入事件
- **AND** `RenderAppShell` SHALL 在下一次 `run_frame` 的 input 阶段调用 `RenderAppHooks::on_input`

#### Scenario: 通道满载不阻塞主线程

- **WHEN** 渲染线程因任何原因长时间未消费事件
- **THEN** 主线程 `send` SHALL 仍然立即返回，不阻塞 winit event loop

### Requirement: 二阶段关闭握手

进程关闭流程 SHALL 保证 winit `Window` 的销毁发生在所有 Vulkan 资源（特别是 `VkSurfaceKHR`）销毁之后。渲染线程观察到退出信号后 SHALL 调用 `RenderApp::shutdown`，由 `RenderAppShell` 先调用 app hooks shutdown，再销毁 backend/Gfx 资源。

#### Scenario: 用户关闭窗口

- **WHEN** 主线程收到 `WindowEvent::CloseRequested`
- **THEN** 主线程 SHALL 设置共享状态的 `exit` 标志
- **AND** 主线程 SHALL NOT 在该回调中调用 `event_loop.exit()`
- **AND** 主线程 SHALL 继续 pump winit 事件

#### Scenario: 渲染线程响应退出

- **WHEN** 渲染线程在下一轮循环开头观察到 `exit` 标志为 true
- **THEN** 渲染线程 SHALL 跳出主循环
- **AND** SHALL 调用 `RenderApp::shutdown`
- **AND** `RenderAppShell` SHALL 执行 `RenderAppHooks::shutdown` 后再销毁 `RenderBackend` 和 `Gfx`
- **AND** SHALL 在所有销毁完成后置位共享状态的 `render_finished` 标志

#### Scenario: 主线程触发 event loop 退出

- **WHEN** 主线程在 `about_to_wait` 回调中观察到 `render_finished` 为 true
- **THEN** 主线程 SHALL 调用 `event_loop.exit()`
- **AND** 在 `run_app` 返回后 SHALL `join` 渲染线程，然后才允许 `Window` drop

#### Scenario: 渲染线程 panic

- **WHEN** 渲染线程的循环体发生 panic
- **THEN** panic SHALL 被 `catch_unwind` 捕获
- **AND** 渲染线程 SHALL 仍然置位 `exit` 与 `render_finished`
- **AND** 主线程 `join` 时 SHALL 通过 `panic::resume_unwind` 重新抛出原始 panic payload
