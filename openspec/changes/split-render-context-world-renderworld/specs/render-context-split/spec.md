## MODIFIED Requirements

### Requirement: RenderContext 定义在 truvis-renderer 中

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

### Requirement: ComputePass::exec 接收具体参数而非 RenderContext

此 requirement 不变。`ComputePass::exec` 继续接收 `&FrameCounter` 和 `&GlobalDescriptorSets`，不接收 `&RenderContext`（已被删除）也不接收 `&RenderWorld`。

#### Scenario: ComputePass::exec 签名只包含实际使用的类型

- **WHEN** 查看 `ComputePass::exec` 的函数签名
- **THEN** 参数列表中包含 `frame_counter: &FrameCounter` 和 `global_descriptor_sets: &GlobalDescriptorSets`，不包含 `render_context` 或 `render_world`
