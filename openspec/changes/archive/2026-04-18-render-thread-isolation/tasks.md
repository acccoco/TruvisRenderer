## 1. 依赖与共享状态骨架

- 1.1 在 workspace `Cargo.toml` 添加 `crossbeam-channel` 依赖，在 `truvis-winit-app/Cargo.toml` 引用
- 1.2 在 `truvis-winit-app` 新建 `shared.rs`，定义 `SharedState`：`exit: AtomicBool`、`render_finished: AtomicBool`、`size: AtomicU64`、`panic_payload: Mutex<Option<Box<dyn Any + Send>>>`、事件 `Sender<InputEvent>` / `Receiver<InputEvent>`（crossbeam unbounded）
- 1.3 在 `shared.rs` 中定义 `SendWrapper<T>` 及 `unsafe impl Send`，附带使用约束注释
- 1.4 定义渲染线程初始化消息 `RenderInitMsg { raw_display: SendWrapper<RawDisplayHandle>, raw_window: SendWrapper<RawWindowHandle>, scale_factor: f64, initial_size: [u32; 2] }` 及其单次 channel

## 2. 事件与 resize 走中转层（仍单线程）

- 2.1 `WinitApp::window_event` 中不再直接调 `RenderApp::handle_event`，改为 `shared.event_sender.send(input_event)`
- 2.2 在 `WindowEvent::Resized` / `ScaleFactorChanged` 中用 `shared.size.store(pack(w, h), Relaxed)` 更新 atomic
- 2.3 在 `RenderApp::big_update` 开头添加 "drain events from channel → `InputManager::push_event`" 与 "检查 size atomic" 两段逻辑；原回调 push 路径删除
  - 实际将 drain 与 size 读取都放在 `render_loop`（渲染线程循环）而非 `big_update`，因为 `big_update` 仅在 `time_to_render` 为真时被调；`big_update` 内原有的 resize/push 路径已删除。
- 2.4 统一 swapchain 重建入口：把现有 `need_resize()` 与 `OUT_OF_DATE` 处理合并到单个函数（比如 `RenderApp::recreate_swapchain_if_needed`），读 atomic 并与 `last_built_size` 对比
- 2.5 本地运行 `cargo run --bin triangle` / `rt-cornell`，确认事件与 resize 行为不回归（仍然单线程跑）
  - 一次性落地到独立线程，未保留「先单线程 drain 再 spawn」的中间提交；`cargo check --bin triangle` 通过，运行时验证并入 7.6。

## 3. 封装渲染循环函数

- 3.1 把 `RenderApp::big_update` 的外层条件与调用点，重构到 `fn render_loop(shared: Arc<SharedState>, init_msg: RenderInitMsg) -> Result<(), RenderLoopExit>` 形式；暂不在新线程运行
  - 实际签名简化为 `fn render_loop(shared, init_msg, outer_app)` 返回 `()`；panic 由上层 `catch_unwind` 捕获并写入 `shared.panic_payload`，无需显式 `RenderLoopExit`。
- 3.2 `render_loop` 内部：接收 init msg → 初始化 `Gfx` + `Renderer` + `init_after_window` → 主循环 → 退出时 `Gfx::wait_idle` + 销毁
- 3.3 `WinitApp` 改为在 `resumed` 中构造 `RenderInitMsg` 并 send 到 init channel，然后（临时）在主线程同步调用 `render_loop` 验证可运行
  - 跳过“同步调用”中间态，直接落地 spawn（见 Phase 4）。

## 4. 真正 spawn 渲染线程

- 4.1 引入 `render_thread: Option<JoinHandle<()>>` 到 `WinitApp`
- 4.2 在 `WinitApp::run` 或首次 `resumed` 中 `thread::Builder::new().name("RenderThread").spawn(...)` 启动渲染线程
- 4.3 渲染线程入口：`tracy_client::set_thread_name!("RenderThread")` → `catch_unwind(AssertUnwindSafe(|| render_loop(...)))` → 无论成败都 set `exit` + `render_finished`，panic payload 存入 `shared.panic_payload`
- 4.4 调整 `WinitApp::init_after_window`：不再直接调 `RenderApp::init_after_window`，改为从主线程 window 取 raw handles → 包 `SendWrapper` → send 到 init channel
  - 未独立开 init channel，而是直接把 `RenderInitMsg` 作为 spawn 闭包捕获传入渲染线程（单次投递语义等价，且省一条依赖）。
- 4.5 移除 `about_to_wait` 中的 `request_redraw` 调用（渲染线程自驱）
- 4.6 渲染线程一定时间未 time_to_render 时 `thread::park_timeout(Duration::from_millis(1))`

## 5. 二阶段关闭握手

- 5.1 `WindowEvent::CloseRequested` 处理：`shared.exit.store(true, Release)`，不再调用 `event_loop.exit()`
- 5.2 渲染线程主循环开头检测 `shared.exit.load(Acquire)`，为 true 则跳出并进入销毁路径
- 5.3 渲染线程销毁完成后 `shared.render_finished.store(true, Release)`
- 5.4 `WinitApp::about_to_wait` 检测 `render_finished`，为 true 时调 `event_loop.exit()`
- 5.5 `WinitApp::destroy`（`run_app` 返回后）`take()` `JoinHandle` 并 `join()`；若 `shared.panic_payload` 有值则 `panic::resume_unwind`
- 5.6 `WinitApp::destroy` 中最后 drop `Window`（确保 join 完成之后）

## 6. 边界与正确性

- 6.1 零尺寸（最小化）处理：atomic 读到 w=0 或 h=0 时，渲染线程 skip 渲染但仍 drain 事件
- 6.2 确认 `OuterApp::init` 在渲染线程调用；检查每个 `truvis-app/src/outer_app/*` 是否隐式依赖主线程（如剪贴板/winit 直接调用），必要时在注释中标记
  - 审查结论：四个内置 `OuterApp`（triangle / cornell / sponza / shader_toy）仅操作 `Renderer` / `Gfx` / imgui `Ui`，未直接调用 winit 或剪贴板 API，迁移到渲染线程安全。
- 6.3 确认 `imgui::Context` / `GuiHost` 仅在渲染线程访问；主线程不再直接调 `gui_host.handle_event`
  - `GuiHost::handle_event` 只在 `RenderApp::big_update`（渲染线程）被调；主线程 `window_event` 只将事件送入 channel。
- 6.4 审查 `RenderApp`、`Renderer`、`Gfx` 相关类型的 `Send` 边界，把阻止 Send 的字段用 `SendWrapper` 或重构解决
  - `RenderApp` / `Renderer` 内部含大量 `Rc`，但全部**只在渲染线程内构造与使用**，不跨线程。跨线程的只有 `Arc<SharedState>`（Send+Sync）、`RenderInitMsg`（`SendWrapper` 提供 Send）以及 `OuterAppFactory`（`FnOnce() -> Box<dyn OuterApp> + Send`，延迟构造规避 `dyn OuterApp: !Send`）。

## 7. 验证与压力测试

- 7.1 启动 + 立即关闭：多次重复，验证无 Vulkan validation error、无 window-before-surface 崩溃
- 7.2 拖动窗口持续 resize 5 秒：确认无崩溃、swapchain 尺寸最终正确；记录是否有肉眼可见卡顿（决定是否需要 open question 1 的 debounce）
- 7.3 窗口最小化 / 还原：确认零尺寸分支正确，还原后能继续渲染
- 7.4 在某个 outer app 的 `init` 或每帧代码中临时 `panic!`，验证主线程能 resume panic、进程正常退出
- 7.5 Tracy 连接确认线程名为 `RenderThread` 且 `frame_mark` 正常
- 7.6 所有示例（`triangle`、`rt-cornell`、`rt-sponza`、`shader-toy`）均能正常启动、运行、关闭

## 8. 文档与收尾

- 8.1 更新 `AGENTS.md` / `.github/copilot-instructions.md` 中涉及 "winit 控制渲染循环" 的描述
  - 已更新 `engine/crates/AGENTS.md`：补充二阶段关闭握手，并修正 `big_update` 时序描述（事件 drain / resize 检查在 `render_loop` 顶部完成）。
  - 已更新 `.github/copilot-instructions.md`：明确主线程仅做事件转发与退出握手，不再以 `request_redraw` 驱动渲染循环。
- 8.2 在 `docs/design/` 加短记（可选），记录此次剥离完成后的新循环结构
  - 已有 `docs/design/render-thread-isolation.md`，包含线程分工、每帧循环、关闭握手与 panic 传播说明。
- 8.3 运行 `openspec validate render-thread-isolation --strict` 确认 change 通过
  - 2026-04-18 本地执行：`openspec validate render-thread-isolation --strict`，结果 `Change 'render-thread-isolation' is valid`。

