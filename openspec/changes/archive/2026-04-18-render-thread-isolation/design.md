## Context

当前 `truvis-winit-app` 的 `WinitApp` 同时持有 `RenderApp`，winit 的 `ApplicationHandler` 回调直接驱动渲染（`window_event(RedrawRequested)` → `RenderApp::big_update()`，`about_to_wait` 无条件 `request_redraw`）。事件经由 winit 回调推送到 `InputManager` 的 `VecDeque`，再在同一次 `big_update` 中被 drain。

这种结构带来几个结构性问题：
- 渲染节奏由 winit 事件分发决定；拖动窗口进入 modal event loop 时渲染和事件派发互相阻塞。
- `Renderer::time_to_render()` 这种"自主 pacing"的钩子实际无处生效。
- 后续 "App 主循环 / 主动 poll 事件 / Renderer 贴近 GPU" 等架构调整都需要先摆脱 winit 对渲染循环的控制。

本次设计只做一件事：把渲染循环从 winit 回调剥离到独立线程，winit 仅作为 event pump。其它职责/抽象层面的重构不在本次范围。

## Goals / Non-Goals

**Goals:**
- 渲染循环在独立 OS 线程中执行；winit 仅在主线程负责 window 生命周期、事件 pump、退出握手。
- 主线程 → 渲染线程的事件传递走 `crossbeam-channel`（unbounded）。
- Resize 通过 `AtomicU64` 打包 `(w, h)` 共享最新尺寸，渲染线程用 "last-seen" 模式消费；与 `VK_ERROR_OUT_OF_DATE_KHR` 合并到同一重建入口。
- 关闭流程二阶段化：主线程不在回调中直接 `event_loop.exit()`，避免 window 先于 surface 销毁。
- Vulkan 对象（`Gfx`、`Renderer`、Surface、Swapchain）**完整**生活在渲染线程；主线程不触碰 Vulkan。
- 渲染线程 panic 不让主线程悄悄黑屏：`catch_unwind` + exit flag + 主线程 `join` 时 `resume_unwind`。

**Non-Goals:**
- 不拆 `Renderer` / `RenderApp` / `OuterApp` 职责。
- 不引入 tick system、不迁移 `CameraController`。
- 不改 `Renderer::time_to_render()` 的驱动模型（保持基于 timer）。
- 不做 resize debounce（记为 Open Question，之后观察体验再决定）。
- 不支持多窗口、不考虑 macOS/Web 的特殊适配（只保 Windows 桌面正常运行；其它平台不做回归）。

## Decisions

### 决策 1：通道选型 —— crossbeam-channel (unbounded)

**选择**：`crossbeam-channel`，事件通道使用 `unbounded`。

**理由**：
- 主线程事件回调必须**非阻塞**，不能因渲染线程慢而卡 winit event loop，`bounded` 的 `send` 可能阻塞，排除。
- 未来渲染线程需要同时等"事件 / resize 信号 / exit 信号"，crossbeam 的 stable `select!` 比 `std::mpsc` 的手动轮询干净。
- 性能上明显优于 `std::mpsc`，开销对单生产单消费场景可忽略。

**备选与放弃理由**：
- `std::mpsc::channel`：可用但无 `select!`；未来扩展成本更高。
- `Arc<Mutex<VecDeque>>`：写代码量最小但并发正确性手工保证，不如 channel 明确。
- `tokio::mpsc`：引入 async 运行时，对非 async 渲染栈不划算。

**风险**：
- `unbounded` 队列在渲染线程长时间停顿（alt-tab/suspend）时可能积压。**缓解**：渲染线程每次 tick 一次性 drain 全部事件；若后续观察到异常积压，再加上限。

### 决策 2：Resize 共享 —— 单个 AtomicU64 打包尺寸 + last-seen 对比

**选择**：主线程在 `WindowEvent::Resized`（及 `ScaleFactorChanged` 触发的尺寸更新）时：
```text
shared.size.store(((w as u64) << 32) | (h as u64), Ordering::Relaxed)
```
渲染线程每次循环开头读取并与自身记录的 `last_built_size` 对比，不同则触发重建。

**理由**：
- 多次连续 resize 天然合并为"最新值"，无需 dirty flag 的三步握手。
- 渲染线程是 resize 的唯一消费者，own `last_built_size` 状态最自然。
- `AtomicU64` 在 64 位平台无锁、无内存屏障开销。
- `VK_ERROR_OUT_OF_DATE_KHR` 返回时走同一 `recreate_swapchain` 入口，重建后把当前读到的 `size` 更新为 `last_built_size`，两条触发路径统一。

**备选**：
- `AtomicBool` dirty flag + 另存尺寸：三步写，易丢信号。
- `Mutex<Option<[u32;2]>>`：可读性更好但引入锁；resize 事件频率（modal loop 时 ~每帧）下锁争用可忽略，主要缺点是"消费后清空"仍需决策。

**DPI (ScaleFactor)**：本次先不在共享状态中单独暴露 `scale_factor`；winit `ScaleFactorChanged` 事件仍会触发 `Resized`（携带新物理尺寸），走同一路径。若后续 HiDPI 相关逻辑需要 factor 本身，再加 `AtomicU64<f64::to_bits>`。

### 决策 3：关闭流程 —— 二阶段握手

关闭涉及三个必须按顺序发生的事件：
1. 渲染线程停止提交新工作并 `Gfx::wait_idle`
2. Vulkan 资源（swapchain、surface、device）销毁
3. winit `Window` 销毁

**流程**：

```text
主线程                              渲染线程
─────                              ──────
WindowEvent::CloseRequested
  │
  └─ shared.exit.store(true)        (loop 开头检查 exit)
                                      │
                                      ├─ break loop
                                      ├─ Gfx::wait_idle()
                                      ├─ Renderer::destroy() 
                                      │    (内含 surface / swapchain)
                                      ├─ Gfx::destroy()
                                      └─ shared.render_finished.store(true)
                                         线程函数返回 → 可被 join
  
about_to_wait {
  if shared.render_finished.load() {
    event_loop.exit();  ← 只此处一处调用
  }
}

run_app 返回
  │
  ├─ join_handle.join().unwrap()  ← 此时线程已 finish，join 立即返回
  │                                  若渲染线程 panic，panic 在此抛出
  │
  └─ drop(Window)                  ← surface 已销毁，安全
```

**理由**：
- `event_loop.exit()` 一旦调用，`run_app` 会在当前回调返回后立刻返回，`WinitApp` 开始析构。若此时渲染线程还持有 surface，window drop 会把 surface 下面的 HWND 销毁，Vulkan 进入未定义行为。
- 用 `render_finished` 作为"允许 exit"的前置条件，可以保证 window 一定比 surface 活得久。
- `CloseRequested` 之后主线程继续 pump 事件，渲染线程可能需要在退出前处理 queue 中剩余事件（虽然通常不关心），`about_to_wait` 每轮检查 finished flag 最多多一轮 pump，代价可忽略。

**备选**：
- 主线程在 `CloseRequested` 里直接 `join`：死锁可能（渲染线程若在等主线程投递的 resize / handle 也会锁死）。
- 渲染线程完成后通过 winit 的 `EventLoopProxy` 发 user event 触发 exit：可行，但多引入一条依赖；轮询 flag 足够简单。

### 决策 4：Window / RawHandle 跨线程 —— 主线程 own Window，通过 channel 投递 handle 给渲染线程

**选择**：
- `Window` 始终留在主线程，不跨线程（winit 明确要求）。
- 主线程在 `resumed` 中创建 `Window` 后，通过一条"初始化 channel"把 `RawDisplayHandle` + `RawWindowHandle` + `scale_factor` + 初始 `[w, h]` 投递给渲染线程。
- 渲染线程接收后用 `ash_window::create_surface` 创建 Vulkan surface，完成 `Gfx::init` + `Renderer::new` + `init_after_window`。

**Send 问题**：
- `RawDisplayHandle` / `RawWindowHandle` 在 `raw-window-handle` 0.5+ 默认 `!Send`。
- 解法：定义本地 `SendWrapper<T>(T)`，`unsafe impl Send for SendWrapper<T> {}`，并在注释中明确"调用者保证 handle 在 window 生命周期内有效"。这是 wgpu / ash-window 生态的常见做法。
- `Window` 本身保留在主线程，不进 wrapper。

**风险**：
- 渲染线程持有的 `RawWindowHandle` 本质是"指向主线程拥有的 HWND 的裸指针"。必须保证 window 销毁晚于 surface 销毁（见决策 3）。
- 主线程在窗口销毁前不能 drop window（除了关闭路径），否则渲染线程会用到野指针。这在当前代码中自然成立（只在 `WinitApp::destroy` 才 drop）。

### 决策 5：渲染线程 pacing —— 沿用 `Renderer::time_to_render()`

**选择**：渲染线程的 loop 大致为
```text
loop {
    if shared.exit.load() { break; }
    drain_events_into_input_manager();
    handle_resize_if_changed();
    if !renderer.time_to_render() {
        thread::park_timeout(SMALL_DURATION);  // 例如 1ms
        continue;
    }
    render_app.big_update();
}
```
`time_to_render()` 的内部逻辑本次不改。

**理由**：专注线程剥离，避免引入 pacing 模型变化造成的额外变量。`park_timeout` 给未来"主线程通过 `unpark` 通知事件到达"留接口，但本次**不实现 unpark**（轮询足够）。

**Non-goal**：基于 fence / vkAcquireNextImageKHR 阻塞驱动 pacing —— 留给后续 change。

### 决策 6：Panic 传播

渲染线程入口：
```text
let result = panic::catch_unwind(AssertUnwindSafe(|| render_loop(...)));
shared.render_finished.store(true);
shared.exit.store(true);  // 通知主线程
if let Err(payload) = result {
    *shared.panic_payload.lock() = Some(payload);
}
```
主线程 `join` 后若发现 `panic_payload` 有值，`panic::resume_unwind(payload)`。

**理由**：渲染线程 panic 若不传播，主线程会在一个"表面正常但再也不渲染"的状态里继续 pump 事件，用户以为卡死。resume 让进程像单线程时一样崩溃退出。

## Risks / Trade-offs

- **[Risk]** 关闭顺序出错导致 window 先于 surface 销毁 → Vulkan UB。  
  **Mitigation**：决策 3 的二阶段握手；集成测试中加入"启动即关闭"、"快速连续 `CloseRequested`" 两个用例。

- **[Risk]** 渲染线程 panic 时主线程阻塞在 `join` 或无限 pump。  
  **Mitigation**：决策 6；渲染线程无论 panic 与否都必须写入 `render_finished` 和 `exit`，保证主线程在 `about_to_wait` 能看到 exit 条件。

- **[Risk]** `RawWindowHandle` 跨线程访问时序不安全。  
  **Mitigation**：生命周期上 window > surface（由决策 3 保证）；wrapper 上加 `unsafe` 注释与 TODO，禁止在 handle 上做除"传给 ash_window"之外的操作。

- **[Risk]** 事件延迟从 0 帧变为最多 1 帧（事件到达 ≠ 渲染线程消费时刻）。  
  **Mitigation**：单帧延迟（~16ms）可接受；若未来对输入延迟敏感，可用 `Thread::unpark` 在主线程投递事件后立即唤醒渲染线程。本次不做。

- **[Risk]** winit 在某些情况（如拖动窗口的 modal loop）事件密度高，channel 吞吐压力。  
  **Mitigation**：crossbeam unbounded 每次 send/recv 为 ns 级，与 winit 事件频率（~kHz 上限）比绰绰有余。不做预防优化。

- **[Trade-off]** 代码量增加（共享状态结构、两个线程入口、二阶段关闭）；不会减少单帧 CPU 开销，也不会自动降低延迟。这是结构性投资，收益在后续 change 兑现。

- **[Trade-off]** 调试复杂度提升：崩溃栈需要看两个线程；Tracy / log 里的线程 ID 变为日常上下文。

## Migration Plan

仅工作区内部改动，无对外 API，无数据迁移。推荐提交顺序（每步都应能编译与运行）：

1. **添加依赖**：workspace `Cargo.toml` 引入 `crossbeam-channel`。
2. **定义共享状态**：新增 `truvis-winit-app::shared::SharedState`（含 `exit`、`render_finished`、`size`、`panic_payload`、事件 sender/receiver），同线程下接通 —— 仍在 `window_event` 回调里消费事件、走 `RenderApp::big_update`，但事件和 resize 经由 `SharedState` 中转。验证事件流与 resize 合并行为。
3. **提取渲染循环入口**：把 `RenderApp::big_update` 的循环体封装成 `render_loop(shared: Arc<SharedState>, init_msg: ...)`，仍在主线程调用。
4. **实际 spawn 渲染线程（空跑）**：渲染线程先不做 Vulkan 初始化，只轮询 exit flag；验证线程生命周期 + 二阶段关闭 + panic 传播路径。
5. **迁移 Vulkan 初始化到渲染线程**：主线程创建 window 后投递 init 消息；渲染线程接收后执行 `Gfx::init` / `Renderer::new` / `init_after_window`。
6. **接通事件与 resize 消费**：渲染线程每轮 drain events + 读取 size atomic。
7. **完善退出路径**：`WinitApp::about_to_wait` 检查 `render_finished`；`WinitApp::destroy` 中 `join`。
8. **压力测试**：启动即关闭；快速拖动窗口；窗口最小化/还原；alt-tab；在 outer app `init` 中人为 panic。

**回滚**：任一步失败，revert 该步提交即可；每步都保留可运行状态。

## Open Questions

1. **Resize debounce 是否需要？**（决策 4 里放弃了首版实现）
   - 拖动窗口时每次都重建 swapchain 在现代 GPU 上是否足够流畅？先跑起来看体验。若卡顿，再加 ~50ms debounce 窗口（在渲染线程实现，不改 shared state）。

2. **主线程是否需要在每次投递事件后主动 `unpark` 渲染线程？**
   - 当前渲染线程 `park_timeout(1ms)` 轮询，最坏输入延迟 +1ms，可接受。
   - 未来若做"渲染 pacing 基于 VSync 阻塞"时，事件延迟问题会突出，届时再引入 unpark。

3. **`OuterApp::init` 是否仍在渲染线程调用？**
   - 当前设计是 **是**（因为 `init_after_window` 挪到渲染线程）。这意味着 OuterApp 的实现者可在 init 中直接访问 Vulkan 资源。但若未来 outer app 需要从主线程拿 winit 扩展能力（剪贴板、窗口 API 等），要加一条"渲染线程 → 主线程"反向通道。暂不处理。

4. **是否在 Renderer 销毁之外增加超时保护？**
   - 若 `Gfx::wait_idle` 因 GPU hang 永久不返回，主线程也会永久等 `render_finished`。当前不做超时（保持和现状行为一致：挂就是挂）。
