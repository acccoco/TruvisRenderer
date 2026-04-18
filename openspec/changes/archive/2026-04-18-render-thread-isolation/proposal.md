## Why

目前 winit 的事件循环控制了整个进程的节奏：`RedrawRequested` 回调里同步执行整套 `RenderApp::big_update()`，`about_to_wait` 里无条件 `request_redraw`。这让渲染节奏和窗口事件分发耦合在同一线程，`Renderer::time_to_render()` 这类"自己决定何时渲染"的意图无处落地；同时窗口拖动等 modal event loop 场景下，GPU 提交会和事件派发互相阻塞。

把渲染循环从 winit 回调中剥离到独立线程，是后续"App 主循环主动驱动 / 事件主动 poll / Renderer 贴近 GPU"等一系列架构调整的前置基建。

## What Changes

- **BREAKING** `WinitApp` 不再直接拥有并同步调用 `RenderApp`；改为在主线程 spawn 一条渲染线程，`RenderApp` 搬迁到该线程执行。
- 主线程职责收窄为：winit event pump、window 生命周期、向渲染线程转发事件与 resize 信号、驱动退出舞步。
- 主线程 → 渲染线程事件传递改走 `crossbeam-channel` 的 `unbounded` 通道，主线程不再直接调用 `RenderApp::handle_event`。
- Resize 通过 `AtomicU64` 共享最新物理尺寸（打包 `(w, h)`），渲染线程用 "last-seen" 模式消费；同一 swapchain 重建入口同时处理 `VK_ERROR_OUT_OF_DATE_KHR`。
- 渲染线程自己 pacing（保留现有 `Renderer::time_to_render()`，不做渲染驱动逻辑的改造）。
- 关闭流程改为二阶段：`CloseRequested` 只置 exit flag；渲染线程退出后置 finished flag；主线程在 `about_to_wait` 轮询 finished flag 后才调 `event_loop.exit()`；`run_app` 返回后再 `join` 渲染线程，最后 drop `Window`，保证 window 在 surface 之后销毁。
- Vulkan 初始化（`Gfx::init`、`Renderer::new`、`init_after_window`）迁到渲染线程执行；主线程创建 `Window` 后把 raw display/window handle 通过 channel 传递（用本地 `SendWrapper` 绕过 `!Send` 约束）。
- Tracy `set_thread_name!("RenderThread")` 迁到渲染线程入口；渲染线程用 `catch_unwind` 包裹，panic 时设置 exit flag 并在主线程 join 时 resume。
- **Non-goal**：本次不拆 `RenderApp` / `Renderer` 职责、不改 `CameraController` 归属、不引入 tick system、不做 resize debounce、不改 `time_to_render` 的驱动模型。这些在后续独立 change 中处理。

## Capabilities

### New Capabilities
- `render-threading`: 定义渲染循环的线程归属、主线程与渲染线程之间的通信契约（事件通道、resize 共享状态、退出握手）、以及跨线程生命周期顺序（Vulkan 对象与 winit `Window` 的销毁先后）。

### Modified Capabilities
<!-- 当前 openspec/specs/ 为空，无既有 capability 需要修改 -->

## Impact

- `truvis-winit-app/src/app.rs`：`WinitApp` 结构拆分，新增渲染线程 spawn / join 路径、共享状态（`Arc<SharedState>`）、二阶段退出逻辑。
- `truvis-winit-app/src/winit_event_adapter.rs`：保持现有 `InputEvent` 抽象，但事件从此 push 到 channel 而非直接调 `RenderApp`。
- `truvis-app/src/render_app.rs`：`RenderApp` 的 `handle_event` 改为从 channel 批量 drain；`init_after_window` 的调用时机从 winit `resumed` 回调移到渲染线程内部（接收主线程投递的 handles 后执行）；`big_update` 改由渲染线程的 loop 驱动。
- `truvis-app/src/platform/input_manager.rs`：保持现有 pull 接口，但事件来源切换为 channel。
- `Cargo.toml` / workspace：新增 `crossbeam-channel` 依赖。
- 渲染线程需要 `Send` 的字段边界：`Renderer`、`Gfx`、`RenderApp` 整体需能在非主线程构造与运行；如有 `!Send` 类型（raw handle、某些 winit 类型），用受控的 `SendWrapper` 处理。
- 不影响 shader 编译、资产加载、具体 Pass 实现、`OuterApp` trait 签名。
