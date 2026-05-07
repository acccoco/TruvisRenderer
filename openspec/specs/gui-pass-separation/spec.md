## Purpose

定义 ImGui 渲染相关职责边界：`GuiRgPass` 作为 render-graph 适配层归属应用侧，`GuiPass` 作为纯 Vulkan 录制层归属 gui-backend，从而保持 gui-backend 与 render-graph 解耦，并清理无关依赖。

## Requirements

### Requirement: GuiRgPass 定义在 truvis-app 中

GUI render-graph adapter SHALL move from demo app code into `GuiPlugin`'s upper integration crate (`truvis-gui-plugin` or equivalent). It SHALL NOT move into low-level `truvis-gui-backend`.

#### Scenario: GuiRgPass hidden behind GuiPlugin

- **WHEN** App needs GUI rendering
- **THEN** App SHALL call `GuiPlugin::contribute_passes`
- **AND** App SHALL NOT manually construct `GuiRgPass`

#### Scenario: GUI render graph adapter has upper-layer ownership

- **WHEN** checking crate ownership
- **THEN** render graph adapter code SHALL live in `truvis-gui-plugin` or an equivalent upper integration crate
- **AND** it SHALL NOT live in `truvis-gui-backend`

### Requirement: GuiPass 保留在 truvis-gui-backend 中

`GuiPass` SHALL remain in `truvis-gui-backend` as the low-level Vulkan command recording component used by the upper `GuiPlugin` integration.

#### Scenario: GuiPlugin reuses GuiPass

- **WHEN** `GuiPlugin` records GUI rendering through RenderGraph
- **THEN** the low-level draw recording SHALL reuse `truvis_gui_backend::gui_pass::GuiPass` or its equivalent backend component

### Requirement: truvis-logs 无未使用依赖

`truvis-logs` 的 `Cargo.toml` SHALL NOT 包含源码中未使用的依赖。

#### Scenario: truvis-logs 无幽灵依赖

- **WHEN** 检查 `engine/crates/truvis-logs/Cargo.toml`
- **THEN** 不存在 `reqwest`、`serde`、`zip`、`toml`、`anyhow` 依赖项

#### Scenario: truvis-logs 编译正常

- **WHEN** 运行 `cargo check -p truvis-logs`
- **THEN** 编译成功

### Requirement: truvis-gui-backend 不依赖 truvis-render-graph

`truvis-gui-backend` SHALL remain independent from `truvis-render-graph`. The new `GuiPlugin` integration crate MAY depend on both `truvis-gui-backend` and `truvis-render-graph` to bridge the two.

#### Scenario: Low-level backend stays graph-free

- **WHEN** checking `truvis-gui-backend` dependencies
- **THEN** there is no dependency on `truvis-render-graph`

#### Scenario: Integration crate bridges GUI backend to RenderGraph

- **WHEN** checking `truvis-gui-plugin` or equivalent
- **THEN** it MAY depend on `truvis-gui-backend`, `truvis-render-graph`, and `truvis-frame-api`
