## Purpose

定义 `RenderContext` 在分层架构中的归属与依赖边界：完整上下文由 `truvis-renderer` 持有，`truvis-render-graph` 保持为与 scene/asset 解耦的渲染编排层，避免跨层依赖回流。

## Requirements

### Requirement: RenderContext 定义在 truvis-renderer 中

`RenderContext` 和 `RenderContext2` 结构体 SHALL 定义在 `truvis-renderer` crate 中（`truvis_renderer::render_context` 模块），而非 `truvis-render-graph`。

#### Scenario: RenderContext 从 truvis-renderer 导入

- **WHEN** 任何 crate 需要使用 `RenderContext` 类型
- **THEN** 该 crate SHALL 从 `truvis_renderer::render_context::RenderContext` 导入

#### Scenario: RenderContext 保持原有字段

- **WHEN** `RenderContext` 被搬迁到 truvis-renderer
- **THEN** 其所有字段（scene_manager、gpu_scene、asset_hub、fif_buffers、bindless_manager 等）SHALL 保持不变

### Requirement: truvis-render-graph 不依赖 truvis-scene 和 truvis-asset

`truvis-render-graph` 的 `Cargo.toml` 中 SHALL NOT 包含对 `truvis-scene` 或 `truvis-asset` 的依赖。

#### Scenario: render-graph Cargo.toml 无 scene/asset 依赖

- **WHEN** 检查 `engine/crates/truvis-render-graph/Cargo.toml`
- **THEN** 不存在 `truvis-scene` 或 `truvis-asset` 依赖项

#### Scenario: render-graph 编译不依赖 scene/asset

- **WHEN** 运行 `cargo check -p truvis-render-graph`
- **THEN** 编译成功且不拉入 truvis-scene 或 truvis-asset

### Requirement: ComputePass::exec 接收具体参数而非 RenderContext

`ComputePass::exec` 方法 SHALL 接收它实际使用的参数（`&FrameCounter` 和 `&GlobalDescriptorSets`），而非完整的 `&RenderContext`。

#### Scenario: ComputePass::exec 签名只包含实际使用的类型

- **WHEN** 查看 `ComputePass::exec` 的函数签名
- **THEN** 参数列表中包含 `frame_counter: &FrameCounter` 和 `global_descriptor_sets: &GlobalDescriptorSets`，不包含 `render_context: &RenderContext`

#### Scenario: ComputePass::exec 功能不变

- **WHEN** 调用 `ComputePass::exec` 传入 frame_counter 和 global_descriptor_sets
- **THEN** 行为与之前传入 `&RenderContext` 完全一致（绑定 pipeline、push constants、descriptor sets、dispatch）
