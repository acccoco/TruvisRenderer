# 渲染线程剥离：循环结构

> 维护状态（2026-04-23）：渲染线程剥离仍是当前主线；
> 但本文“兼容入口”部分提到的 `OuterApp` / `LegacyOuterAppAdapter` 在当前代码中已不再保留，
> 当前入口是 `WinitApp::run_plugin(|| Box<dyn AppPlugin>)`。
> 线程分工、共享状态和二阶段关闭握手仍可作为当前设计说明阅读。

本笔记记录 `render-thread-isolation` change 落地后，winit 主线程与渲染线程的分工与握手。

> 术语更新（`frame-runtime-boundary-refactor` 之后）：
> - `RenderApp` -> `FrameRuntime`
> - `OuterApp` -> `AppPlugin`
> - `WinitApp::run`（兼容）-> `WinitApp::run_plugin`（默认）

## 线程分工

| 线程 | 职责 |
| --- | --- |
| **winit 主线程** | `EventLoop` pump、`Window` 生命周期、事件翻译转发、resize atomic 写、二阶段退出握手 |
| **渲染线程** (`RenderThread`) | Vulkan 初始化与销毁、`FrameRuntime` 全部状态、每帧 pacing、`AppPlugin::init`/`build_ui`/`update`/`render`、`GuiHost` 访问 |

Vulkan 对象（`Gfx` 单例、`Renderer`、`VkSurfaceKHR`、swapchain、command buffers、fences、semaphores）严格只在渲染线程创建与销毁。主线程不直接调用任何 `ash` / `truvis-gfx` API。

## 共享状态

`truvis-winit-app/src/shared.rs::SharedState` 通过 `Arc<SharedState>` 在两个线程间共享：

- `exit: AtomicBool` — 主线程置位，渲染线程每轮开头 `Acquire` 读
- `render_finished: AtomicBool` — 渲染线程销毁完成后置位，主线程在 `about_to_wait` 观察
- `size: AtomicU64` — 打包 `(w << 32) | h`，Relaxed 读写；多次连续 resize 天然合并为最新值
- `panic_payload: Mutex<Option<Box<dyn Any + Send>>>` — 渲染线程 `catch_unwind` 落脚点
- `event_sender` / `event_receiver`：`crossbeam_channel::unbounded::<InputEvent>()`，单生产单消费

跨线程传递 `RawDisplayHandle` / `RawWindowHandle` 通过 `SendWrapper<T>`（`unsafe impl Send`），生命周期由二阶段关闭保证。

## 启动流程

```
winit 主线程                           RenderThread
─────────────                          ────────────
WinitApp::run_plugin(factory)
  ├── FrameRuntime::init_env()
  └── EventLoop::run_app → resumed
        ├── create_window
        ├── Arc::new(SharedState)
        └── thread::spawn ─────────▶ tracy_client::set_thread_name!
                                       catch_unwind:
                                         plugin = factory()
                                         render_loop(shared, init_msg, plugin)
                                          ├── FrameRuntime::new_with_plugin
                                          ├── init_after_window
                                          └── loop { ... }
```

## 每帧 (RenderThread)

```
loop {
    if shared.exit.load(Acquire) { break; }

    while let Ok(ev) = shared.event_receiver.try_recv() {
        render_app.input_manager.push_event(ev);
    }

    let [w, h] = unpack_size(shared.size.load(Relaxed));
    if w == 0 || h == 0 {
        thread::park_timeout(1ms);          // 最小化：skip 渲染，仍 drain 事件
        continue;
    }

    render_app.recreate_swapchain_if_needed([w, h], &mut last_built_size);

    if !renderer.time_to_render() {
        thread::park_timeout(1ms);
        continue;
    }

    render_app.big_update();                 // begin_frame / acquire / draw / present / end_frame
}
```

`big_update` 内部不再处理 resize 事件，也不再判断 `need_resize` — 这两件事完全由 `render_loop` 上游处理。事件 drain 也提前到 loop 顶部，`big_update` 内只剩"转发给 imgui + 推进 `InputManager` 状态机"。

## 二阶段关闭握手

```
主线程                                 渲染线程
──────                                 ──────
WindowEvent::CloseRequested
  └── shared.exit.store(true, Release)

                                       loop 顶部 exit = true → break
                                       render_app.destroy()
                                         ├── Gfx::wait_idle
                                         ├── Renderer::destroy (surface/swapchain)
                                         └── Gfx::destroy
                                       spawn wrapper:
                                         exit.store(true); render_finished.store(true)

about_to_wait {
  if shared.render_finished { event_loop.exit(); }
}

run_app 返回
  └── WinitApp::destroy()
        ├── render_thread.join()       ← 线程已 finish
        ├── drop(Window)               ← 此时 surface 已销毁，安全
        └── if panic_payload.take().is_some() { panic::resume_unwind(p); }
```

关键不变量：`Window` 的生命周期一定覆盖 `VkSurfaceKHR`。`event_loop.exit()` 只有一个调用点（`about_to_wait`），且以 `render_finished` 为前置条件。

## Panic 传播

渲染线程 spawn wrapper 用 `catch_unwind(AssertUnwindSafe(|| render_loop(...)))`。无论是否 panic：

1. `render_finished` 与 `exit` 必定被置位 → 主线程 `about_to_wait` 一定能观察退出条件，不会卡死。
2. 若 panic，payload 存入 `shared.panic_payload`；主线程 `destroy()` 在 join 完成后 `panic::resume_unwind(payload)`，把渲染线程的崩溃抛回主线程，让进程正常崩溃退出而不是"黑屏但事件还在 pump"。

## 入口契约（当前与兼容）

当前默认入口：

```rust
pub fn run_plugin<F>(plugin_factory: F)
where F: FnOnce() -> Box<dyn AppPlugin> + Send + 'static;
```

兼容入口（deprecated）：

```rust
pub fn run<F>(outer_app_factory: F)
where F: FnOnce() -> Box<dyn OuterApp> + Send + 'static;
```

两种入口都采用工厂闭包形式，由渲染线程在 `spawn` 内构造应用对象，避免把应用对象本体跨线程传输。
兼容入口内部通过 `LegacyOuterAppAdapter` 包装到 `AppPlugin`。

## 不在本次 change 的范围（历史记录）

- `FrameRuntime` / `Renderer` / `AppPlugin` 职责拆分（后续已由 `frame-runtime-boundary-refactor` 推进）
- tick system、`CameraController` 归属
- `Renderer::time_to_render()` 驱动模型变化（仍基于 timer 阈值）
- resize debounce（由实际体验触发）
- 主线程 → 渲染线程的 `unpark` 唤醒优化
