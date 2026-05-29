# truvis-streamline-binding

`truvis-streamline-binding` 是 Streamline / DLSS 的 Rust 侧绑定 crate。它负责三件事：

- 通过 bindgen 使用 `truvixx-streamline-capi` 暴露的稳定 C ABI。
- 用 `StreamlineRuntime` 管理进程级 `slInit` / `slShutdown` 生命周期。
- 把 Streamline 的 `logMessageCallback` 接入项目统一的 Rust `log` facade。

本 crate 只面向 Windows x64，不提供跨平台抽象。Streamline SDK 的路径参数使用
null-terminated UTF-16，Vulkan loader 也按 Windows DLL 路径处理。

## 源码边界

- `src/lib.rs`：crate 入口，只声明模块并维持对外 re-export，不放具体生命周期实现。
- `src/config.rs`：Streamline 初始化参数、默认路径和 Vulkan loader 路径 helper。
- `src/runtime.rs`：`StreamlineRuntime`、`StreamlineError` 和 `slInit/slShutdown` RAII 生命周期。
- `src/log_bridge.rs`：Streamline log callback 到 Rust `log` facade 的异步桥接。

## 日志链路

Streamline 日志不是由 C++ wrapper 直接输出，而是被转成 Rust 日志事件：

```text
Streamline logMessageCallback
  -> C++ sl_log_callback
  -> Rust extern "C" callback
  -> bounded queue
  -> streamline-log-drain thread
  -> log::debug! / log::warn! / log::error!
  -> truvis-logs / env_logger
```

可以把这条链路理解成一条传送带：C++ 负责从 Streamline 手里接过日志，Rust callback
负责快速投递，`streamline-log-drain` 线程才是真正写入 Rust 日志系统的一端。

如果应用入口没有初始化 `truvis_logs::init_log()` 或其他 `log` backend，日志事件仍会进入
`log` facade，但不会自动显示。这和项目里其他 `log::info!` / `log::warn!` 调用一致。

## C++ 侧职责

C++ wrapper 始终向 Streamline 注册内部 `sl_log_callback`。这个 callback 只做轻量工作：

- 当收到 `eError` 时更新 wrapper 内部的 `last_error`，供 `truvixx_sl_last_error_utf8()` 查询。
- 把 `sl::LogType` 映射成稳定的 `TruvixxSlLogType`，避免 Rust 直接依赖 Streamline C++ ABI。
- 使用 `GetCurrentThreadId()` 记录 SL callback 发生时的 native thread id。
- 通过 Rust 传入的 `TruvixxSlLogCallback` 和 `log_user_data` 转发日志事件。

C++ 侧不使用 `OutputDebugStringA` fallback。没有 Rust callback 时，非错误日志会被丢弃；
错误日志只保留在 `last_error` 中。

## Rust 侧职责

Rust callback 是 FFI 边界上的快速入口，它不直接做最终日志输出。它只负责：

- 检查 C++ 传入的指针是否为空。
- 立即把 `message_utf8 + message_len` 复制成 Rust `String`，不保存 C++ 字符串指针。
- 把日志事件通过 `try_send` 放入容量为 1024 的 bounded queue。
- 用 `catch_unwind` 包住 callback，防止 Rust panic 跨过 C ABI 边界。

callback 中不要调用 SL API，不要执行文件 IO，不要直接做复杂日志格式化。原因是 SL callback
可能发生在初始化、关闭、渲染线程或 Vulkan interposer 调用栈中；这些位置都不适合被日志
系统的锁或 IO 阻塞。

## drain 线程

`StreamlineLogBridge` 创建时会启动名为 `streamline-log-drain` 的线程。它从 queue 中取出
日志事件，并统一调用 Rust `log` facade：

```text
SL Info  -> log::debug!(target: "streamline", ...)
SL Warn  -> log::warn!(target: "streamline", ...)
SL Error -> log::error!(target: "streamline", ...)
```

日志内容会附带 `[sl_tid=...]`。这是 C++ 侧记录的 native thread id，表示 SL 原始 callback
发生在哪个线程上。最终 Rust formatter 看到的线程通常是 `streamline-log-drain`，所以
`sl_tid` 用来补充真实来源线程。

## 队列满策略

日志队列是 bounded queue，容量固定为 1024。callback 使用 `try_send`，队列满时不会等待，
而是丢弃新日志并增加 dropped counter。

drain 线程后续会输出一条 warn，说明丢弃了多少条 Streamline 日志。这个策略的目标是保护
SL/Vulkan/render 调用栈：日志可以丢，但 callback 不能因为日志系统繁忙而阻塞渲染线程。

## 线程安全与生命周期

Streamline runtime 是进程级单例，日志桥也跟随 `StreamlineRuntime` 的生命周期：

```text
StreamlineRuntime::init
  -> 创建 StreamlineLogBridge
  -> 把 callback 和 user_data 传给 C++
  -> C++ 调用 slInit

StreamlineRuntime::drop
  -> 先调用 truvixx_sl_shutdown()
  -> C++ 在 slShutdown 返回后清空 callback / user_data
  -> Rust 释放 sender
  -> drain 线程 flush 已入队日志并退出
  -> join drain 线程
```

`log_user_data` 指向 Rust 持有的日志桥状态。它必须活到 `slShutdown` 之后，因为 shutdown
期间 Streamline 仍可能输出日志。`StreamlineRuntime` 持有 `StreamlineLogBridge`，正是为了
保证这段裸指针生命周期覆盖整个 SL runtime。

C++ callback 复制 callback 指针和 `user_data` 后，会在释放 C++ mutex 之后再调用 Rust
callback。这样可以避免 Rust 日志桥或 channel 操作反向卡住 Streamline 生命周期锁。

## 关键约束

- Rust callback 不能 panic 跨 FFI。
- Rust callback 不能调用 SL API。
- Rust callback 不能直接做最终 IO。
- C++ 不负责最终日志输出。
- `truvixx_sl_last_error_utf8()` 只表示最近错误，不承担日志流职责。
- 业务侧统一通过 `log` facade 接收日志，具体输出格式由 `truvis-logs` 维护。
