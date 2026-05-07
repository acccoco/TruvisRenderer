# plugin-trait Specification

## Purpose
TBD - created by archiving change plugin-system-baseapp. Update Purpose after archive.
## Requirements
### Requirement: Plugin trait 定义统一生命周期契约

系统 SHALL 在 `truvis-frame-api` 中定义 `Plugin` trait，包含以下生命周期 hook，所有 hook SHALL 提供默认空实现：

- `init(&mut self, ctx: &mut PluginInitCtx)` — 一次性初始化
- `on_input(&mut self, event: &InputEvent) -> bool` — 处理输入事件，返回是否消费
- `update(&mut self, ctx: &mut PluginUpdateCtx)` — CPU 侧逻辑更新
- `on_resize(&mut self, ctx: &mut PluginResizeCtx)` — swapchain 重建后重建 GPU 资源
- `shutdown(&mut self)` — 释放资源

#### Scenario: Plugin 可只实现需要的 hook

- **WHEN** 一个 Plugin 只关心 render graph 贡献（无 input / update 需求）
- **THEN** 该 Plugin 实现 `Plugin` trait 时只需 override `init` 和 `shutdown`，其余 hook 使用默认空实现

#### Scenario: Plugin trait 在 truvis-frame-api crate 中定义

- **WHEN** 外部 crate 需要实现 Plugin
- **THEN** SHALL 从 `truvis_frame_api` 导入 `Plugin` trait

### Requirement: Plugin 特有方法通过具体类型暴露

Plugin 的特有能力（如 GUI 的 `ui()` 方法、渲染管线的 `contribute_passes()` 方法）SHALL 作为具体 struct 的方法暴露，而非加入 `Plugin` trait。App SHALL 通过持有 Plugin 的具体类型来调用这些方法。

#### Scenario: App 通过具体类型调用特有方法

- **WHEN** App 需要调用 `GuiPlugin::ui()` 获取 imgui 上下文
- **THEN** App 通过持有的 `GuiPlugin` 具体类型字段直接调用 `self.gui.ui()`
- **AND** 不需要 downcast 或类型擦除

#### Scenario: 特有方法不在 Plugin trait 中

- **WHEN** 检查 `Plugin` trait 定义
- **THEN** trait 中 SHALL NOT 包含 `contribute_passes`、`build_ui`、`ui()` 等特有方法

### Requirement: PluginInitCtx 提供初始化所需的完整上下文

`PluginInitCtx` SHALL 包含 `&mut World`、`&mut RenderWorld`、`&mut CmdAllocator`、`GfxSwapchainImageInfo`、`&RenderPresent`，以满足 Plugin 初始化阶段创建 GPU 资源的需求。

#### Scenario: Plugin 在 init 中创建 GPU 资源

- **WHEN** Plugin 的 `init` 被调用
- **THEN** 可通过 `PluginInitCtx` 访问 `RenderWorld`（descriptor sets、bindless manager）和 `CmdAllocator`（分配 command buffer）

### Requirement: PluginUpdateCtx 提供 CPU 更新所需上下文

`PluginUpdateCtx` SHALL 包含 `&mut World`、`&mut PipelineSettings`、`&FrameSettings`、`delta_time_s: f32`，以满足 Plugin 每帧 CPU 侧更新需求。

#### Scenario: Plugin 在 update 中修改场景数据

- **WHEN** Plugin 的 `update` 被调用
- **THEN** 可通过 `PluginUpdateCtx` 读写 `World` 中的场景数据和 `PipelineSettings`

### Requirement: PluginRenderCtx 不包含 gui_draw_data

`PluginRenderCtx` SHALL 包含 `&RenderWorld`、`&RenderPresent`、`&GfxSemaphore`（timeline），SHALL NOT 包含 `gui_draw_data`。GUI 渲染数据由 `GuiPlugin` 自己管理。

#### Scenario: RenderCtx 无 GUI 数据泄漏

- **WHEN** 检查 `PluginRenderCtx` 的字段
- **THEN** SHALL NOT 存在 `gui_draw_data` 或任何 imgui 相关类型

### Requirement: PluginResizeCtx 提供 resize 后重建所需上下文

`PluginResizeCtx` SHALL 包含 `&mut RenderWorld`、`&RenderPresent`，以满足 Plugin 在 swapchain 重建后重新创建尺寸相关 GPU 资源的需求。

#### Scenario: Plugin 在 on_resize 中重建资源

- **WHEN** Plugin 的 `on_resize` 被调用
- **THEN** 可通过 `PluginResizeCtx` 访问新的 swapchain 信息（通过 `RenderPresent`）和 `RenderWorld`

### Requirement: on_input 返回值表示事件消费

`Plugin::on_input` SHALL 返回 `bool`，`true` 表示事件已被此 Plugin 消费。App 可根据返回值决定是否将事件传递给后续 Plugin。

#### Scenario: GUI Plugin 消费鼠标事件

- **WHEN** GuiPlugin 的 `on_input` 收到鼠标点击事件且 imgui 要求捕获鼠标
- **THEN** 返回 `true`
- **AND** App 可选择不再将此事件传给 camera controller
