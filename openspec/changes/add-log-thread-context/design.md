## Context

项目当前通过 `truvis-logs::init_log()` 统一初始化 `env_logger`，formatter 位于 `engine/crates/truvis-logs/src/lib.rs`。日志记录已经包含时间、级别、消息、模块和源码位置，但缺少线程上下文；在 winit 主线程、`RenderThread` 和后台 IO 线程同时产生日志时，排查线程归属需要依赖消息内容猜测。

线程信息应当在日志层集中补齐。各业务 crate 继续使用 `log` facade，不应在每个调用点手动拼接线程名或 tid。

## Goals / Non-Goals

**Goals:**

- 每条由 `truvis-logs` formatter 输出的日志都包含线程名称和线程 id。
- 线程名称和线程 id 在线程第一次写日志时获取并缓存，后续日志复用缓存结果。
- 保持现有日志初始化入口和 `log` 调用 API 不变。
- 保持 `truvis-logs` 位于 L0，不引入对渲染层、winit 层或 app 层的依赖。

**Non-Goals:**

- 不替换 `env_logger` / `log` 为 `tracing`。
- 不新增每个模块或每个线程的日志配置系统。
- 不保证日志中的 tid 一定等同于操作系统调试器显示的 native tid；初版只要求它能稳定区分当前进程内的 Rust 线程。
- 不在本次改动中统一命名所有后台线程。

## Decisions

1. **在 `truvis-logs` 内集中实现线程上下文格式化。**

   日志格式由 `truvis-logs` 统一控制，改动 formatter 可以覆盖主 app、shader-build、cxx-build、fetch-res 等所有使用 `init_log()` 的入口。相比修改所有 `log::info!` 调用点，这个方案改动面更小，也不会污染业务消息。

2. **使用 thread-local 缓存线程信息。**

   新增轻量 `ThreadLogContext`，保存格式化后的线程名称和 tid 字符串，并通过 `thread_local!` 在线程第一次输出日志时初始化。这样每条日志只需要一次 TLS 访问和字符串写入，不重复查询 `std::thread::current()`、不重复分配线程名称字符串。

   ```text
   log record
     -> formatter
        -> THREAD_LOG_CONTEXT.with(...)
           -> first log on thread: capture name + tid
           -> later logs: reuse cached strings
   ```

3. **线程名称使用 Rust thread name，未命名线程使用固定占位。**

   `std::thread::current().name()` 能直接读取 `thread::Builder::name()` 设置的名称。现有渲染线程已经命名为 `RenderThread`，主线程和第三方线程可能没有名称；未命名时使用类似 `unnamed` 的稳定占位，避免日志字段为空。

4. **tid 初版使用 Rust 线程唯一标识的可显示形式。**

   Rust `ThreadId` 是跨平台、无依赖、进程内唯一的线程标识，满足日志区分线程来源的主要目标。实现时应把 tid 获取封装在独立函数中；如果后续需要与 Visual Studio、WinDbg、Linux perf 等工具中的 OS native tid 对齐，可以在该函数内替换为平台实现，而不影响 formatter 和缓存结构。

## Risks / Trade-offs

- **tid 不等同于 OS native tid** -> 初版文档明确 tid 的语义是日志系统线程唯一标识；如调试器对齐成为明确需求，再增加平台实现。
- **线程重命名后缓存不更新** -> Rust 线程名称通常在线程创建时确定，项目现有模型也是如此；缓存按这个约束设计。
- **日志行变长影响可读性** -> 将线程上下文放在时间和级别附近，并保持源码位置仍在第二行，降低对现有阅读习惯的影响。
- **formatter 高频路径增加开销** -> thread-local 缓存避免重复字符串分配，剩余成本主要是 TLS 访问和写入字段。
