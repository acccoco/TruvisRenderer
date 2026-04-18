## Purpose

定义渲染循环的线程归属、主线程与渲染线程之间的通信契约（事件通道、resize 共享状态、退出握手），以及跨线程生命周期顺序（Vulkan 对象与 winit `Window` 的销毁先后），使渲染节奏与 winit 事件泵解耦并保证资源安全。

## Requirements

### Requirement: 渲染循环运行于独立线程

渲染循环 SHALL 在独立于 winit 主线程的 OS 线程中执行。winit 主线程仅负责 window 生命周期与事件 pump，不得直接调用 `FrameRuntime::big_update` 或任何 Vulkan API。

#### Scenario: 进程启动

- **WHEN** `WinitApp::run_plugin`（或兼容入口 `WinitApp::run`）被调用
- **THEN** 主线程创建 winit `EventLoop`；在 `resumed` 回调中创建 `Window` 后，spawn 一条渲染线程并传递 window handles
- **AND** 渲染线程内部完成 `Gfx::init` / `Renderer::new` / `init_after_window`
- **AND** 主线程后续仅执行事件 pump 与退出握手

#### Scenario: 每帧渲染由渲染线程自主驱动

- **WHEN** 渲染线程处于主循环中
- **THEN** 渲染线程 SHALL 自行决定何时推进帧（通过 `Renderer::time_to_render()` 或等价机制），不依赖 winit 的 `RedrawRequested` 事件
- **AND** 主线程 SHALL NOT 再调用 `Window::request_redraw`

### Requirement: 事件通过 crossbeam-channel 传递

主线程 SHALL 把 winit `WindowEvent` 翻译为 `InputEvent` 后，通过 `crossbeam-channel` 的 unbounded 通道投递给渲染线程。

#### Scenario: 普通输入事件

- **WHEN** winit 在主线程上发出 `KeyboardInput` / `MouseInput` / `MouseMotion` 等事件
- **THEN** 主线程 SHALL 翻译为 `InputEvent` 并非阻塞地 `send` 到事件通道
- **AND** 渲染线程 SHALL 在每轮主循环开头一次性 drain 通道中所有事件，灌入 `InputManager`

#### Scenario: 通道满载不阻塞主线程

- **WHEN** 渲染线程因任何原因长时间未消费事件
- **THEN** 主线程 `send` SHALL 仍然立即返回，不阻塞 winit event loop

### Requirement: Resize 通过 AtomicU64 共享最新尺寸

主线程 SHALL 通过单个 `AtomicU64`（高 32 位为 width，低 32 位为 height）发布最新的物理尺寸；渲染线程 SHALL 使用 "last-seen" 模式消费。

#### Scenario: 连续多次 resize 合并

- **WHEN** 主线程在渲染线程一次循环间隔内连续收到多个 `Resized` 事件
- **THEN** 渲染线程下一轮循环 SHALL 只看到最后一次写入的尺寸，并 **最多** 触发一次 swapchain 重建

#### Scenario: 重建路径与 OUT_OF_DATE 合并

- **WHEN** `vkAcquireNextImageKHR` 返回 `VK_ERROR_OUT_OF_DATE_KHR` 或 `VK_SUBOPTIMAL_KHR`
- **THEN** 渲染线程 SHALL 走与 resize 相同的 swapchain 重建入口（单一入口函数）
- **AND** 重建完成后 SHALL 更新 `last_built_size` 为当前 atomic 中的值

#### Scenario: 零尺寸忽略

- **WHEN** atomic 中读取到 width 或 height 为 0（窗口最小化）
- **THEN** 渲染线程 SHALL 跳过本轮 swapchain 重建与渲染，但继续消费事件

### Requirement: 二阶段关闭握手

进程关闭流程 SHALL 保证 winit `Window` 的销毁发生在所有 Vulkan 资源（特别是 `VkSurfaceKHR`）销毁之后。

#### Scenario: 用户关闭窗口

- **WHEN** 主线程收到 `WindowEvent::CloseRequested`
- **THEN** 主线程 SHALL 设置共享状态的 `exit` 标志，且 SHALL NOT 在该回调中调用 `event_loop.exit()`
- **AND** 主线程 SHALL 继续 pump winit 事件

#### Scenario: 渲染线程响应退出

- **WHEN** 渲染线程在下一轮循环开头观察到 `exit` 标志为 true
- **THEN** 渲染线程 SHALL 跳出主循环
- **AND** SHALL 依次执行 `Gfx::wait_idle` → 销毁所有 Vulkan 资源
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

所有 Vulkan 对象（`Gfx`、`Renderer`、`VkSurfaceKHR`、swapchain、command buffer、fence、semaphore 等）SHALL 仅在渲染线程中创建、使用和销毁。主线程 SHALL NOT 直接调用任何 `ash` / `truvis-gfx` API。

#### Scenario: Vulkan 初始化位置

- **WHEN** `Gfx::init` / `Renderer::new` / `Renderer::init_after_window` 被调用
- **THEN** 调用 SHALL 发生在渲染线程中
- **AND** 主线程仅负责把 `RawDisplayHandle` / `RawWindowHandle` / 初始尺寸通过通道投递给渲染线程

#### Scenario: RawWindowHandle 跨线程

- **WHEN** 主线程需要把 `RawWindowHandle` / `RawDisplayHandle` 传给渲染线程
- **THEN** 实现 SHALL 使用受控的 `SendWrapper` 将 handle 标记为 `Send`
- **AND** SHALL 由关闭流程保证 `Window` 生命周期覆盖 `VkSurfaceKHR` 生命周期

### Requirement: 渲染线程命名与可观测性

渲染线程 SHALL 具备可辨识的线程名，供调试器和 profiler 识别。

#### Scenario: Tracy 与日志中的线程名

- **WHEN** 渲染线程启动
- **THEN** SHALL 调用 `tracy_client::set_thread_name!("RenderThread")`
- **AND** 线程在操作系统层面的名称 SHALL 被设置为 `"RenderThread"` 或等价标识
