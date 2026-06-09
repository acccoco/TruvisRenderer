# truvis-streamline-binding

`truvis-streamline-binding` 是 Streamline / DLSS 的 Rust 侧绑定 crate。它负责三件事：

- 通过 bindgen 使用 `truvixx-streamline-capi` 暴露的稳定 C ABI。
- 用 `StreamlineRuntime` 管理进程级 `slInit` / `slShutdown` 生命周期。
- 把 Streamline 的 `logMessageCallback` 接入项目统一的 Rust `log` facade。

本 crate 只面向 Windows x64，不提供跨平台抽象。Streamline SDK 的路径参数使用
null-terminated UTF-16，Vulkan loader 也按 Windows DLL 路径处理。Rust 侧会把
`sl.interposer.dll` 的绝对路径传给 C++ wrapper；C++ 使用 `LoadLibraryW` /
`GetProcAddress` 显式解析 `slInit` / `slShutdown`，不通过链接期导入库自动加载
Streamline runtime。

## 源码边界

- `src/lib.rs`：crate 入口，只声明模块并维持对外 re-export，不放具体生命周期实现。
- `src/config.rs`：Streamline 初始化参数、默认路径、feature flags 和 Vulkan loader 路径 helper。
- `src/dlss.rs`：DLSS SR/RR support query、options、resource tags、evaluate 和 resource free 的 Rust 安全封装。
- `src/runtime.rs`：`StreamlineRuntime`、`StreamlineError` 和 `slInit/slShutdown` RAII 生命周期。
- `src/log_bridge.rs`：Streamline log callback 到 Rust `log` facade 的异步桥接。

## 日志链路

Streamline 日志不是由 C++ wrapper 直接输出，而是被转成 Rust 日志事件：

```text
Streamline logMessageCallback
  -> C++ sl_log_callback
  -> Rust extern "C" callback
  -> bounded queue
  -> streamline-logger thread
  -> log::debug! / log::warn! / log::error!
  -> truvis-logs / env_logger
```

可以把这条链路理解成一条传送带：C++ 负责从 Streamline 手里接过日志，Rust callback
负责快速投递，`streamline-logger` 线程才是真正写入 Rust 日志系统的一端。

如果应用入口没有初始化 `truvis_logs::init_log()` 或其他 `log` backend，日志事件仍会进入
`log` facade，但不会自动显示。这和项目里其他 `log::info!` / `log::warn!` 调用一致。

## C++ 侧职责

C++ wrapper 不静态导入 `sl.interposer.dll`。`truvixx_sl_init` 会先使用 Rust 传入的
`interposer_dll_path_utf16` 加载同一份 `sl.interposer.dll`，再解析 `slInit` /
`slShutdown` 并组装 `sl::Preferences`。随后 wrapper 始终向 Streamline 注册内部
`sl_log_callback`，该 callback 只做轻量工作：

- 把 `sl::LogType` 映射成稳定的 `TruvixxSlLogType`，避免 Rust 直接依赖 Streamline C++ ABI。
- 把 Rust 传入的稳定 feature flags 翻译成 Streamline SDK 的 feature id。
- 把 DLSS SR/RR 的 C++ SDK types 包装成稳定 C ABI POD，Rust 只传 Vulkan handles 和显式资源契约。
- 通过 Rust 传入的全局 `TruvixxSlLogCallback` 转发日志事件。
- 在 `LoadLibraryW` / `GetProcAddress` 失败时，通过同一条日志链路输出路径和 Win32 错误。

C++ 侧不使用 `OutputDebugStringA` fallback。没有 Rust callback 时，日志会被丢弃。

## Rust 侧职责

Rust callback 是 FFI 边界上的快速入口，它不直接做最终日志输出。它只负责：

- 检查 C++ 传入的指针是否为空。
- 立即把 `message_utf8 + message_len` 复制成 Rust `String`，不保存 C++ 字符串指针。
- 把日志事件通过 `try_send` 放入容量为 1024 的 bounded queue。
- 用 `catch_unwind` 包住 callback，防止 Rust panic 跨过 C ABI 边界。

callback 中不要调用 SL API，不要执行文件 IO，不要直接做复杂日志格式化。原因是 SL callback
可能发生在初始化、关闭、渲染线程或 Vulkan interposer 调用栈中；这些位置都不适合被日志
系统的锁或 IO 阻塞。

Streamline feature 选择由 `StreamlineInitInfo` 负责：默认请求 DLSS SR + DLSS RR，Debug
可通过 `TRUVIS_STREAMLINE_IMGUI` 显式请求 SL ImGui 调试 UI；Release 中该环境变量会被
warning 后忽略。C++ wrapper 不根据构建类型做策略判断。

## drain 线程

`StreamlineLogBridge` 创建时会启动名为 `streamline-logger` 的线程。它从 queue 中取出
日志事件，并统一调用 Rust `log` facade：

```text
SL Info  -> log::debug!(target: "streamline", ...)
SL Warn  -> log::warn!(target: "streamline", ...)
SL Error -> log::error!(target: "streamline", ...)
```

最终 Rust formatter 看到的线程是 `streamline-logger`；SL 原始 callback 可能来自 init、
shutdown、渲染线程或 Vulkan interposer 调用栈，因此 callback 内只做复制和入队。

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
  -> 把 callback、plugin 目录、interposer DLL 路径传给 C++
  -> C++ 显式加载 sl.interposer.dll 并调用 slInit

StreamlineRuntime::drop
  -> 先调用 truvixx_sl_shutdown()
  -> C++ 在 slShutdown 返回后 FreeLibrary 并清空 callback
  -> Rust 向全局 sender 发送 Shutdown
  -> drain 线程 flush 已入队日志并退出
  -> join drain 线程
```

`StreamlineRuntime` 持有 `StreamlineLogBridge`，保证 Rust 日志入口活到 `slShutdown`
之后，因为 shutdown 期间 Streamline 仍可能输出日志。C++ 侧只保存函数指针，不持有 Rust
对象指针。生产路径下 Streamline 初始化失败会由 `Gfx::new` 直接 panic，当前不支持同一进程内
重复初始化 Streamline；日志 sender 因此使用一次性全局 `OnceLock`。

## 关键约束

- Rust callback 不能 panic 跨 FFI。
- Rust callback 不能调用 SL API。
- Rust callback 不能直接做最终 IO。
- C++ 不负责最终日志输出。
- C++ wrapper 不静态链接 `sl.interposer.lib`，只能通过 Rust 传入的绝对路径加载 SL DLL。
- SR/RR evaluate 是 opaque external command；Rust/app 层负责 RenderGraph resource state、
  pass 顺序和 GPU idle 后再释放旧 feature resources。
- Streamline 初始化失败是启动失败，不做同进程 runtime retry。
- 业务侧统一通过 `log` facade 接收日志，具体输出格式由 `truvis-logs` 维护。
