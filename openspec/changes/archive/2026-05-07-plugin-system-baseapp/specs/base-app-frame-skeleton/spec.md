## ADDED Requirements

### Requirement: BaseApp 持有 RenderBackend 和输入事件队列

`BaseApp` struct SHALL 持有 `RenderBackend` 和待处理 `InputEvent` 队列作为帧骨架的基础设施。`BaseApp` SHALL NOT 持有 Plugin、Camera、GUI context、InputState 或任何 app 特定状态。

#### Scenario: BaseApp 不知道 Plugin 的存在

- **WHEN** 检查 `BaseApp` 的字段和方法签名
- **THEN** SHALL NOT 存在对 `Plugin` trait、`GuiPlugin`、`Camera` 或任何具体 Plugin 类型的引用

#### Scenario: BaseApp 持有帧基础设施

- **WHEN** `BaseApp` 被构造
- **THEN** 它 SHALL 持有已初始化的 `RenderBackend` 和空的输入事件队列

### Requirement: BaseApp::run_frame 提供不变的帧骨架

`BaseApp` SHALL 提供 `pub fn run_frame(&mut self, app: &mut impl FrameAppHooks)` 方法，按以下固定顺序执行帧：

1. `render_backend.begin_frame()`
2. drain input events → `app.on_input(events)`
3. `render_backend.update_phase()` → `app.update(update_ctx)` → drop update_ctx
4. `render_backend.prepare(app.camera())`
5. `render_backend.render_phase()` → `app.render(render_ctx)` → drop render_ctx
6. `render_backend.present()`
7. `render_backend.end_frame()`

#### Scenario: 帧骨架顺序严格固定

- **WHEN** `BaseApp::run_frame` 被调用
- **THEN** RenderBackend 生命周期方法和 App hook 按上述顺序执行，不可重排

#### Scenario: Ctx 的 borrow 语义保持

- **WHEN** `render_backend.update_phase()` 返回的 Ctx 未 drop
- **THEN** `render_backend.prepare()` 等后续方法 SHALL NOT 被调用（编译期保证）

#### Scenario: App hook 在正确的 Ctx 生命期内被调用

- **WHEN** `app.update()` 被调用
- **THEN** 它 SHALL 在 `RenderBackendUpdateCtx` 存活期间执行
- **AND** `app.render()` SHALL 在 `RenderBackendRenderCtx` 存活期间执行

### Requirement: FrameApp trait 面向 render_loop

`FrameApp` trait SHALL 定义 render_loop 调用 App 所需的外部契约：

- `init_after_window(&mut self, raw_display: RawDisplayHandle, raw_window: RawWindowHandle, scale_factor: f64, window_size: [u32; 2])` — 窗口创建后初始化 BaseApp（RenderBackend + surface + swapchain）并初始化所有 Plugin
- `run_frame(&mut self)` — 执行一帧（内部调用 BaseApp）
- `push_input_event(&mut self, event: InputEvent)` — 缓存输入事件
- `recreate_swapchain_if_needed(&mut self, new_size: [u32; 2])` — resize 判定与处理
- `time_to_render(&self) -> bool` — 帧节流查询
- `shutdown(&mut self)` — 销毁 Plugin 然后销毁 BaseApp

#### Scenario: render_loop 只通过 FrameApp 驱动 App

- **WHEN** 渲染线程主循环推进单帧
- **THEN** SHALL 通过 `Box<dyn FrameApp>` 的方法驱动（push_input_event / recreate_swapchain_if_needed / time_to_render / run_frame）
- **AND** render_loop SHALL NOT 直接访问 RenderBackend、BaseApp 或 App 内部字段

#### Scenario: FrameApp::run_frame 内部调用 BaseApp

- **WHEN** App 实现 `FrameApp::run_frame`
- **THEN** SHALL 取出内部 `BaseApp`，调用 `base.run_frame(self)`，调用完毕后放回

#### Scenario: init_after_window 编排 BaseApp 和 Plugin 初始化

- **WHEN** render_loop 在窗口创建后调用 `app.init_after_window(...)`
- **THEN** App SHALL 先调用 `base.init_after_window(...)` 获得 `RenderBackendInitCtx`
- **AND** 再从 `RenderBackendInitCtx` 构造 `PluginInitCtx` 传给每个 Plugin 的 `init`
- **AND** 最后 drop `RenderBackendInitCtx` 解锁 RenderBackend

#### Scenario: time_to_render 委托给 BaseApp

- **WHEN** render_loop 调用 `app.time_to_render()`
- **THEN** App SHALL 委托给 `base.time_to_render()`（BaseApp 内部调 `render_backend.time_to_render()`）

### Requirement: FrameAppHooks trait 面向 BaseApp

`FrameAppHooks` trait SHALL 定义 BaseApp 在帧骨架中回调 App 的 hook 点：

- `on_input(&mut self, events: &[InputEvent])` — 处理输入
- `update(&mut self, ctx: &mut RenderBackendUpdateCtx)` — CPU 更新 + UI 编排
- `render(&mut self, ctx: &RenderBackendRenderCtx)` — GUI mesh 准备、GPU 命令录制 + RenderGraph 构建
- `camera(&self) -> &Camera` — 为 prepare 阶段提供 camera

#### Scenario: App 在 update hook 中顺序编排 Plugin

- **WHEN** `FrameAppHooks::update` 被 BaseApp 调用
- **THEN** App 可在此 hook 中顺序调用 Plugin 的 `update` 方法和特有方法（如 GUI 的 `begin_frame` / `end_frame`）
- **AND** 因为 `PluginUpdateCtx` 含 `&mut` 引用，每次 Plugin 调用 SHALL 在前一次调用的 Ctx 释放后进行（Rust borrow 语义天然保证）

#### Scenario: App 在 render hook 中构建 RenderGraph

- **WHEN** `FrameAppHooks::render` 被 BaseApp 调用
- **THEN** App 可在此 hook 中以可变方式准备需要 per-frame GPU buffer 的 Plugin（如 `GuiPlugin::prepare_render_data`）
- **AND** App 可创建 `RenderGraphBuilder`，调用各 Plugin 的 `contribute_passes`，compile 并 execute

### Requirement: App 通过 Option take 模式解决 borrow 冲突

App SHALL 以 `Option<BaseApp>` 持有 `BaseApp`。在 `FrameApp::run_frame` 中通过 `.take()` 取出 `BaseApp`，调用 `base.run_frame(self)` 后放回。

#### Scenario: take 和 put 在同一函数内完成

- **WHEN** `FrameApp::run_frame` 执行
- **THEN** `base.take()` 和 `self.base = Some(base)` SHALL 在同一函数调用内完成
- **AND** 外部不可观察到 `base` 为 `None` 的中间状态

### Requirement: BaseApp 提供 init_after_window 方法

`BaseApp` SHALL 提供 `init_after_window` 方法，调用 `RenderBackend::init_after_window` 创建 surface/swapchain 并返回 `RenderBackendInitCtx`。App 在收到 Ctx 后自行构造 `PluginInitCtx` 传给 Plugin。

#### Scenario: init_after_window 返回 Ctx 供 App 使用

- **WHEN** App 调用 `base.init_after_window(raw_display, raw_window, window_size)`
- **THEN** BaseApp 内部调用 `render_backend.init_after_window(...)` 并返回 `RenderBackendInitCtx`
- **AND** App 可从 Ctx 中构造 `PluginInitCtx` 传给各 Plugin

### Requirement: BaseApp 提供 time_to_render 方法

`BaseApp` SHALL 提供 `time_to_render(&self) -> bool` 方法，委托给 `RenderBackend::time_to_render()`。

#### Scenario: 帧节流查询委托

- **WHEN** App 调用 `base.time_to_render()`
- **THEN** 返回 `render_backend.time_to_render()` 的结果

### Requirement: BaseApp 提供 push_input_event 方法

`BaseApp` SHALL 提供 `push_input_event(&mut self, event: InputEvent)` 方法，将事件缓存到内部输入事件队列中，供 `run_frame` 内的 `on_input` hook 消费。

#### Scenario: 输入事件缓存到事件队列

- **WHEN** render_loop 收到输入事件并调用 `app.push_input_event(event)`
- **THEN** App 委托给 `base.push_input_event(event)`
- **AND** 事件 SHALL 在下一次 `run_frame` 的 `on_input` hook 中被 drain
- **AND** BaseApp SHALL NOT 将事件累计为 `InputState`；GUI 消费、Camera 输入状态和快捷键等由 App 持有的 Plugin/组件处理

### Requirement: BaseApp 处理 resize 的 RenderBackend 交互

`BaseApp` SHALL 提供 resize 相关方法，将 RenderBackend 的 `handle_resize` 结果返回给 App。如果 RenderBackend 返回 `Some(ResizeCtx)`，App SHALL 将其裁剪为 `PluginResizeCtx` 并传给需要 resize 的 Plugin。

#### Scenario: Resize 只在实际重建时通知 App

- **WHEN** BaseApp 调用 `render_backend.handle_resize(new_size)` 返回 `None`
- **THEN** SHALL NOT 通知 App 执行 resize 相关 Plugin 更新

#### Scenario: Resize 发生时 App 可更新 Plugin

- **WHEN** BaseApp 调用 `render_backend.handle_resize(new_size)` 返回 `Some(ctx)`
- **THEN** App 可遍历 Plugin 调用 `on_resize(ctx)` 重建尺寸相关资源

### Requirement: BaseApp 提供 destroy 方法

`BaseApp` SHALL 提供 `destroy(self)` 方法，按以下顺序执行：`Gfx::wait_idle()` → `render_backend.destroy()` → `Gfx::destroy()`。App SHALL 在调用 BaseApp destroy 之前完成所有 Plugin 的 shutdown。

#### Scenario: shutdown 顺序正确

- **WHEN** App 被销毁
- **THEN** 执行顺序 SHALL 为：App 调用每个 Plugin 的 `shutdown()` → BaseApp `destroy()`（含 RenderBackend 和 Gfx 销毁）
