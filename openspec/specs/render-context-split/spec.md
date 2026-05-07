## Purpose

定义 `RenderContext` 在分层架构中的归属与依赖边界：完整上下文由 `truvis-render-backend` 持有，`truvis-render-graph` 保持为与 scene/asset 解耦的渲染编排层，避免跨层依赖回流。
## Requirements
### Requirement: RenderContext 定义在 truvis-render-backend 中

`RenderContext` 和 `RenderContext2` 结构体 SHALL 被删除。其职责由 `World`（定义在 `truvis-world`）和 `RenderWorld`（定义在 `truvis-render-interface`）接管。

#### Scenario: RenderContext 不再存在

- **WHEN** 搜索 workspace 中的 `struct RenderContext` 定义
- **THEN** SHALL NOT 找到任何结果

#### Scenario: RenderContext2 不再存在

- **WHEN** 搜索 workspace 中的 `struct RenderContext2` 定义
- **THEN** SHALL NOT 找到任何结果

#### Scenario: 原 RenderContext 的字段被 World 和 RenderWorld 完整覆盖

- **WHEN** 对比原 `RenderContext` 的字段列表与新 `World` + `RenderWorld` 的字段列表
- **THEN** 原有全部字段 SHALL 在 `World` 或 `RenderWorld` 中存在，无遗漏

### Requirement: truvis-render-graph 不依赖 truvis-scene 和 truvis-asset

`truvis-render-graph` 的 `Cargo.toml` 中 SHALL NOT 包含对 `truvis-scene` 或 `truvis-asset` 的依赖。

#### Scenario: render-graph Cargo.toml 无 scene/asset 依赖

- **WHEN** 检查 `engine/crates/truvis-render-graph/Cargo.toml`
- **THEN** 不存在 `truvis-scene` 或 `truvis-asset` 依赖项

#### Scenario: render-graph 编译不依赖 scene/asset

- **WHEN** 运行 `cargo check -p truvis-render-graph`
- **THEN** 编译成功且不拉入 truvis-scene 或 truvis-asset

### Requirement: ComputePass::exec 接收具体参数而非 RenderContext

`ComputePass::exec` SHALL 继续接收 `&FrameCounter` 和 `&GlobalDescriptorSets`，SHALL NOT 接收 `&RenderContext`（已被删除）或 `&RenderWorld`。

#### Scenario: ComputePass::exec 签名只包含实际使用的类型

- **WHEN** 查看 `ComputePass::exec` 的函数签名
- **THEN** 参数列表中包含 `frame_counter: &FrameCounter` 和 `global_descriptor_sets: &GlobalDescriptorSets`，不包含 `render_context` 或 `render_world`
