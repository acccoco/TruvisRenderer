## ADDED Requirements

### Requirement: GuiRgPass 定义在 truvis-app 中
`GuiRgPass` 结构体及其 `impl RgPass` SHALL 定义在 `truvis-app` crate 中，而非 `truvis-gui-backend`。

#### Scenario: GuiRgPass 从 truvis-app 导入
- **WHEN** 需要在 render graph 中注册 ImGui pass
- **THEN** SHALL 从 `truvis-app` 中的模块导入 `GuiRgPass`

#### Scenario: GuiRgPass 功能不变
- **WHEN** `GuiRgPass` 被注册为 render graph pass
- **THEN** 其 `setup` 方法正确声明 canvas_color 为 COLOR_ATTACHMENT_READ_WRITE，其 `execute` 方法正确调用 `GuiPass::draw` 完成 ImGui 渲染

### Requirement: truvis-gui-backend 不依赖 truvis-render-graph
`truvis-gui-backend` 的 `Cargo.toml` 中 SHALL NOT 包含对 `truvis-render-graph` 的依赖。

#### Scenario: gui-backend Cargo.toml 无 render-graph 依赖
- **WHEN** 检查 `engine/crates/truvis-gui-backend/Cargo.toml`
- **THEN** 不存在 `truvis-render-graph` 依赖项

#### Scenario: gui-backend 编译不依赖 render-graph
- **WHEN** 运行 `cargo check -p truvis-gui-backend`
- **THEN** 编译成功且不拉入 truvis-render-graph

### Requirement: GuiPass 保留在 truvis-gui-backend 中
纯 Vulkan 录制的 `GuiPass` 结构体 SHALL 继续定义在 `truvis-gui-backend::gui_pass` 模块中。

#### Scenario: GuiPass 不受搬迁影响
- **WHEN** 其他 crate 需要使用 ImGui Vulkan 渲染能力
- **THEN** SHALL 从 `truvis_gui_backend::gui_pass::GuiPass` 导入，API 不变

### Requirement: truvis-logs 无未使用依赖
`truvis-logs` 的 `Cargo.toml` SHALL NOT 包含源码中未使用的依赖。

#### Scenario: truvis-logs 无幽灵依赖
- **WHEN** 检查 `engine/crates/truvis-logs/Cargo.toml`
- **THEN** 不存在 `reqwest`、`serde`、`zip`、`toml`、`anyhow` 依赖项

#### Scenario: truvis-logs 编译正常
- **WHEN** 运行 `cargo check -p truvis-logs`
- **THEN** 编译成功
