## 1. 日志线程上下文实现

- [ ] 1.1 在 `truvis-logs` 中新增 `ThreadLogContext` 辅助结构，保存已格式化的线程名称和 tid。
- [ ] 1.2 使用 `thread_local!` 为每个线程缓存 `ThreadLogContext`，确保线程上下文只在该线程首次输出日志时捕获。
- [ ] 1.3 将线程名称捕获封装为独立函数，命名线程使用 Rust thread name，未命名线程使用稳定占位名称。
- [ ] 1.4 将 tid 捕获封装为独立函数，初版使用 Rust 线程唯一标识的可显示形式，并保留未来替换为 OS native tid 的局部扩展点。
- [ ] 1.5 更新 `env_logger` formatter，在日志主行中输出线程名称和 tid，同时保留现有时间、级别、消息、模块和源码位置。

## 2. 验证与测试

- [ ] 2.1 为线程上下文捕获与缓存逻辑添加聚焦单元测试，覆盖命名线程、未命名线程和同线程复用缓存。
- [ ] 2.2 运行 `cargo fmt --package truvis-logs` 格式化 Rust 代码。
- [ ] 2.3 运行 `cargo test -p truvis-logs` 验证日志 crate。
- [ ] 2.4 运行一个依赖 `truvis-logs` 的现有入口检查编译，例如 `cargo check -p truvis-frame-runtime` 或等价目标。

## 3. 文档

- [ ] 3.1 为 `truvis-logs` 补充模块说明文档，记录日志格式包含线程名称和 tid，以及线程上下文按线程缓存的约束。
- [ ] 3.2 检查 `ARCHITECTURE.md` / 上层 README 是否需要补充日志层说明；若无必要，在实现记录中说明无需更新。
