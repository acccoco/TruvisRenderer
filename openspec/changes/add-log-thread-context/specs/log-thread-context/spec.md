## ADDED Requirements

### Requirement: 日志记录包含线程上下文

`truvis-logs` SHALL 在每条由其 formatter 输出的日志中包含当前线程的可读线程名称和线程 id。

#### Scenario: 命名线程输出日志

- **WHEN** 名称为 `RenderThread` 的线程通过 `log` facade 输出一条日志
- **THEN** formatter 输出 SHALL 包含线程名称 `RenderThread`
- **AND** formatter 输出 SHALL 包含当前线程的线程 id 字段

#### Scenario: 未命名线程输出日志

- **WHEN** 未设置线程名称的线程通过 `log` facade 输出一条日志
- **THEN** formatter 输出 SHALL 包含稳定的未命名线程占位名称
- **AND** formatter 输出 SHALL 包含当前线程的线程 id 字段

### Requirement: 线程上下文按线程缓存

`truvis-logs` SHALL 按线程缓存格式化日志所需的线程名称和线程 id，避免每次 formatter 调用都重复查询和分配线程上下文。

#### Scenario: 同一线程连续输出日志

- **WHEN** 同一线程连续输出多条日志
- **THEN** 该线程的日志 formatter SHALL 复用已缓存的线程名称和线程 id
- **AND** 线程上下文捕获 SHALL 至多在该线程第一次输出日志时执行一次

#### Scenario: 不同线程输出日志

- **WHEN** 两个不同线程分别输出日志
- **THEN** 每个线程 SHALL 拥有独立的线程上下文缓存
- **AND** 一个线程的缓存 SHALL NOT 覆盖另一个线程的线程名称或线程 id

### Requirement: 日志调用 API 保持不变

线程上下文 SHALL 由 `truvis-logs` formatter 自动注入，业务代码 SHALL NOT 需要在 `log::info!`、`log::debug!`、`log::warn!` 或 `log::error!` 调用点手动传入线程信息。

#### Scenario: 现有日志调用继续工作

- **WHEN** 现有代码调用 `log::info!("message")`
- **THEN** 代码 SHALL 无需修改即可编译
- **AND** 输出日志 SHALL 自动包含线程名称和线程 id
