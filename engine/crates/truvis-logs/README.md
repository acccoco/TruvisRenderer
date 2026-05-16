# truvis-logs

`truvis-logs` 提供项目统一日志初始化入口和 `env_logger` formatter。

## 主要职责

- `init_log()` 初始化 `env_logger`，供 engine、tooling 和 app 入口复用。
- formatter 输出时间、级别、线程上下文、消息、模块路径和源码位置。
- `ThreadLogContext` 在线程第一次写日志时捕获线程名称和 tid，并通过 thread-local 缓存复用。

## 线程上下文

- 线程名称优先使用 Rust thread name；未命名线程使用稳定占位名称。
- tid 初版使用 Rust `ThreadId` 的可显示形式，用于在当前进程内区分线程。
- tid 捕获被封装在独立函数中；如果后续需要对齐 OS native tid，可以局部替换实现，不影响业务日志调用点。

## 边界约束

- 业务 crate 继续使用 `log` facade，不在每个 `log::info!` / `log::debug!` 调用点手动拼接线程信息。
- 本 crate 位于基础层，不依赖 winit、render backend、frame runtime 或 App 业务模块。
- 线程上下文按线程缓存；如果线程创建后再改名，缓存不会自动更新。
