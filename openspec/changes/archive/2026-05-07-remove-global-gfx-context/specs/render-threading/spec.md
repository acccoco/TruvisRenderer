## MODIFIED Requirements

### Requirement: 渲染循环运行于独立线程

渲染循环 SHALL 在独立于 winit 主线程的 OS 线程中执行。winit 主线程仅负责 window 生命周期与事件 pump，不得直接调用 `FrameRuntime::run_frame` 或任何 Vulkan API。

#### Scenario: 进程启动

- **WHEN** `WinitApp::run_plugin`（或兼容入口 `WinitApp::run`）被调用
- **THEN** 主线程创建 winit `EventLoop`；在 `resumed` 回调中创建 `Window` 后，spawn 一条渲染线程并传递 window handles
- **AND** 渲染线程内部完成 `RenderBackend::new` / `RenderBackend::init_after_window`
- **AND** `RenderBackend` SHALL 构造并持有 `Gfx` root owner，不通过全局 `Gfx::init`
- **AND** 主线程后续仅执行事件 pump 与退出握手

#### Scenario: 每帧渲染由渲染线程自主驱动

- **WHEN** 渲染线程处于主循环中
- **THEN** 渲染线程 SHALL 自行决定何时推进帧（通过 `RenderBackend::time_to_render()` 或等价机制），不依赖 winit 的 `RedrawRequested` 事件
- **AND** 主线程 SHALL NOT 再调用 `Window::request_redraw`

### Requirement: 二阶段关闭握手

进程关闭流程 SHALL 保证 winit `Window` 的销毁发生在所有 Vulkan 资源（特别是 `VkSurfaceKHR`）销毁之后。

#### Scenario: 用户关闭窗口

- **WHEN** 主线程收到 `WindowEvent::CloseRequested`
- **THEN** 主线程 SHALL 设置共享状态的 `exit` 标志，且 SHALL NOT 在该回调中调用 `event_loop.exit()`
- **AND** 主线程 SHALL 继续 pump winit 事件

#### Scenario: 渲染线程响应退出

- **WHEN** 渲染线程在下一轮循环开头观察到 `exit` 标志为 true
- **THEN** 渲染线程 SHALL 跳出主循环
- **AND** SHALL 依次执行 backend/Gfx idle 等待、App/Plugin GPU shutdown、RenderBackend child resource destroy、`Gfx` root owner destroy
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

### Requirement: Vulkan 资源严格线程局部

所有 Vulkan 对象（`Gfx` root owner、`RenderBackend`、`VkSurfaceKHR`、swapchain、command buffer、fence、semaphore 等）SHALL 仅在渲染线程中创建、使用和销毁。主线程 SHALL NOT 直接调用任何 `ash` / `truvis-gfx` API。

#### Scenario: Vulkan 初始化位置

- **WHEN** `RenderBackend::new` / `RenderBackend::init_after_window` 被调用
- **THEN** 调用 SHALL 发生在渲染线程中
- **AND** `Gfx` root owner SHALL 在渲染线程内由 `RenderBackend` 构造和持有
- **AND** 主线程仅负责把 `RawDisplayHandle` / `RawWindowHandle` / 初始尺寸通过通道投递给渲染线程

#### Scenario: RawWindowHandle 跨线程

- **WHEN** 主线程需要把 `RawWindowHandle` / `RawDisplayHandle` 传给渲染线程
- **THEN** 实现 SHALL 使用受控的 `SendWrapper` 将 handle 标记为 `Send`
- **AND** SHALL 由关闭流程保证 `Window` 生命周期覆盖 `VkSurfaceKHR` 生命周期
