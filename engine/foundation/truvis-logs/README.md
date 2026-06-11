# truvis-logs

`truvis-logs` 提供项目统一日志初始化入口和 `env_logger` formatter。

## 主要职责

- `TruvisLogger::init()` 初始化 `env_logger`，供 engine、tooling 和 app 入口复用。
- `TruvisLogger::init_with_file()` 初始化 console + file 双输出；调用方传入文件路径，本 crate 不直接解析 workspace 路径。
- `LogFilePath::current_exe()` 根据当前 exe 名称生成 `.temp/logs/{exe}-{time}-{pid}.log` 风格的默认路径。
- formatter 输出时间、级别、线程上下文、消息、模块路径和源码位置。
- `ThreadLogContext` 在线程第一次写日志时捕获线程名称和 tid，并通过 thread-local 缓存复用。

## 文件日志

- 文件日志使用 `env_logger::Target::Pipe` 接入内部 tee writer，同一条日志同时写入 console 和文件。
- 双输出模式会让 formatter 生成 ANSI style；console 端通过 `anstream` 适配终端能力，file 端通过 `StripStream`
  移除 ANSI escape sequence，保证文件日志保持纯文本。
- `.temp` 目录由调用方通过 `TruvisPath::temp_dir()` 传入；这样 `truvis-logs` 保持路径无关。
- 日志文件名包含当前 exe 名称、时间和 pid，不同 app、sample、tooling exe 会自然区分。
- 成功创建文件日志后，会按 exe 名称分组保留最近 3 个 `.log` 文件；清理旧日志失败只输出 `eprintln!`，
  不影响当前日志初始化。
- 创建目录或打开文件失败时会退回 console-only，并通过 `eprintln!` 输出失败原因。

## 线程上下文

- 线程名称优先使用 Rust thread name；未命名线程使用稳定占位名称。
- tid 使用 Windows native thread id，用于在当前进程内区分线程；formatter 输出形如 `[main(123)]`。
- tid 捕获被封装在独立函数中；如果后续需要对齐 OS native tid，可以局部替换实现，不影响业务日志调用点。

## 边界约束

- 业务 crate 继续使用 `log` facade，不在每个 `log::info!` / `log::debug!` 调用点手动拼接线程信息。
- 本 crate 位于基础层，不依赖 winit、render backend、frame runtime 或 App 业务模块。
- 线程上下文按线程缓存；如果线程创建后再改名，缓存不会自动更新。
