## Purpose

定义渲染循环的线程归属、主线程与渲染线程之间的通信契约（事件通道、resize 共享状态、退出握手），以及跨线程生命周期顺序（Vulkan 对象与 winit `Window` 的销毁先后），使渲染节奏与 winit 事件泵解耦并保证资源安全。
## Requirements
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

关闭流程 SHALL 保持主线程与渲染线程二阶段握手。渲染线程观察到退出信号后 SHALL 调用 `FrameApp::shutdown(&mut self)`，由 App 关闭 Plugin 并销毁 BaseApp/RenderBackend/Gfx。

#### Scenario: App owns shutdown sequencing

- **WHEN** 渲染线程准备退出
- **THEN** SHALL 调用 `app.shutdown()`
- **AND** App SHALL 先 shutdown Plugin，再 destroy BaseApp
- **AND** 主线程仍 SHALL 等待渲染线程完成后再 drop Window

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

### Requirement: 渲染线程命名与可观测性

渲染线程 SHALL 具备可辨识的线程名，供调试器和 profiler 识别。

#### Scenario: Tracy 与日志中的线程名

- **WHEN** 渲染线程启动
- **THEN** SHALL 调用 `tracy_client::set_thread_name!("RenderThread")`
- **AND** 线程在操作系统层面的名称 SHALL 被设置为 `"RenderThread"` 或等价标识

### Requirement: App and plugin GPU resources SHALL be released before Gfx teardown

During render-thread shutdown, app-owned and plugin-owned GPU resources SHALL be released on the render thread before `RenderBackend` is destroyed and before `Gfx::destroy()` is called. After `Gfx::destroy()` begins, no remaining app/plugin resource `Drop` implementation may call `Gfx::get()` or any Vulkan/VMA destruction API through the project wrappers.

#### Scenario: Render thread shuts down app-owned plugins

- **WHEN** the render loop observes the exit flag and calls `RenderApp::shutdown`
- **THEN** the app hooks and standard plugin shutdown traversal SHALL receive typed shutdown context where manager-owned resources need backend manager access
- **AND** they SHALL release all app/plugin-owned GPU resources while `Gfx` and `RenderBackend` are still alive
- **AND** `RenderBackend::destroy()` SHALL run only after that release phase
- **AND** `Gfx::destroy()` SHALL run only after `RenderBackend::destroy()` completes

#### Scenario: Plugin releases manager-owned resources

- **WHEN** a plugin owns bindless registrations or handles to resources stored in `GfxResourceManager`
- **THEN** `Plugin::shutdown` or the equivalent typed shutdown traversal SHALL expose the required `RenderWorld` manager access
- **AND** the plugin SHALL unregister bindless references before destroying the associated manager-owned images or views
- **AND** the manager SHALL perform image-view-before-image destruction ordering

#### Scenario: App value drops after Gfx teardown

- **WHEN** the concrete app value is later dropped by Rust after `RenderApp::shutdown` has returned
- **THEN** no remaining app/plugin field drop SHALL require `Gfx::get()`
- **AND** debug builds SHALL surface a lifecycle violation if an app/plugin GPU owner was not released during shutdown

### Requirement: 事件通过 crossbeam-channel 传递

事件仍 SHALL 通过 crossbeam-channel 从 winit 主线程传递到渲染线程。渲染线程 SHALL 将事件推入 `FrameApp::push_input_event`，由 App/BaseApp 在下一帧输入 hook 中处理。

#### Scenario: 输入事件交给 FrameApp

- **WHEN** 渲染线程从 channel drain 到 `InputEvent`
- **THEN** SHALL 调用 `app.push_input_event(event)`
- **AND** SHALL NOT 调用 FrameRuntime API
