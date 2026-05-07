## MODIFIED Requirements

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

### Requirement: truvis-gui-backend 不依赖 truvis-render-graph

`truvis-gui-backend` SHALL remain independent from `truvis-render-graph`. The new `GuiPlugin` integration crate MAY depend on both `truvis-gui-backend` and `truvis-render-graph` to bridge the two.

#### Scenario: Low-level backend stays graph-free

- **WHEN** checking `truvis-gui-backend` dependencies
- **THEN** there is no dependency on `truvis-render-graph`

#### Scenario: Integration crate bridges GUI backend to RenderGraph

- **WHEN** checking `truvis-gui-plugin` or equivalent
- **THEN** it MAY depend on `truvis-gui-backend`, `truvis-render-graph`, and `truvis-frame-api`

### Requirement: GuiPass 保留在 truvis-gui-backend 中

`GuiPass` SHALL remain in `truvis-gui-backend` as the low-level Vulkan command recording component used by the upper `GuiPlugin` integration.

#### Scenario: GuiPlugin reuses GuiPass

- **WHEN** `GuiPlugin` records GUI rendering through RenderGraph
- **THEN** the low-level draw recording SHALL reuse `truvis_gui_backend::gui_pass::GuiPass` or its equivalent backend component
