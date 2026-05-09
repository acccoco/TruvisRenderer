## Why

当前日志只能看到时间、级别、模块和源码位置，排查主线程、渲染线程和后台 IO 线程交错问题时缺少线程上下文。渲染项目已经有明确的线程边界，日志中展示线程名称和 tid 可以让跨线程生命周期、资源销毁和 panic 诊断更直接。

## What Changes

- 在日志输出中加入当前线程信息，至少包含线程名称和线程 id。
- 在线程第一次输出日志时缓存线程信息，避免每条日志重复执行可能较重的线程查询和字符串分配。
- 保持现有 `log` facade 调用方式不变，不要求修改各处 `log::info!` / `log::debug!` 调用点。
- 保持 `truvis-logs` 作为日志格式化入口，不引入渲染层到日志层的反向依赖。

## Capabilities

### New Capabilities

- `log-thread-context`: 定义日志记录应包含可读线程上下文，并要求线程信息按线程缓存。

### Modified Capabilities

- 无。

## Impact

- 主要影响 `engine/crates/truvis-logs` 的 formatter 和内部辅助结构。
- 可能影响日志文本快照或人工阅读格式，但不改变 `log` crate 的调用 API。
- 不改变 winit 主线程、RenderThread 或后台 IO 线程的生命周期模型。
- 预计不需要新增运行时配置；如实现 OS tid 可能需要小范围平台条件编译或轻量依赖评估。
