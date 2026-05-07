## ADDED Requirements

### Requirement: GuiPlugin 封装完整的 GUI 能力

`GuiPlugin` struct SHALL 封装 imgui context 管理、input forwarding、font 初始化、GPU mesh buffer 管理和 render graph pass 贡献。它 SHALL 实现 `Plugin` trait 并额外暴露 GUI 特有方法。

#### Scenario: GuiPlugin 实现 Plugin trait

- **WHEN** App 批量调用 Plugin 生命周期方法
- **THEN** `GuiPlugin` 的 `init` / `on_input` / `shutdown` 等 hook 正常执行

#### Scenario: GuiPlugin 暴露 GUI 特有方法

- **WHEN** App 需要编排 GUI 帧
- **THEN** 可调用 `GuiPlugin::begin_frame(delta_time)`、`GuiPlugin::ui() -> &imgui::Ui`、`GuiPlugin::end_frame()`
- **AND** 这些方法 SHALL NOT 在 `Plugin` trait 中定义

### Requirement: GuiPlugin 管理 imgui context 生命周期

`GuiPlugin` SHALL 持有 `imgui::Context`，负责 new_frame / render / end_frame 的调用。`begin_frame` 开启一个 imgui frame，`ui()` 返回当前 frame 的 `&imgui::Ui`，`end_frame` 结束 frame 并生成 draw data。GPU mesh 上传 SHALL 在 render hook 内通过 `prepare_render_data` 单独执行。

#### Scenario: begin_frame 和 end_frame 配对

- **WHEN** App 在 update hook 中调用 `gui.begin_frame(dt)`
- **THEN** 后续调用 `gui.ui()` 返回有效的 `&imgui::Ui`
- **AND** 调用 `gui.end_frame()` 后 draw data 可用于 render 阶段的 `prepare_render_data`

#### Scenario: 未调用 begin_frame 时 ui() 不可用

- **WHEN** App 未调用 `gui.begin_frame()`
- **THEN** 调用 `gui.ui()` SHALL panic 或返回错误

### Requirement: GuiPlugin 消费 input 事件

`GuiPlugin` 的 `Plugin::on_input` 实现 SHALL 将输入事件转发给 imgui context，并根据 imgui 的 want_capture_mouse / want_capture_keyboard 返回是否消费事件。

#### Scenario: 鼠标在 imgui 窗口上时消费事件

- **WHEN** imgui 报告 want_capture_mouse 为 true
- **THEN** `on_input` 对鼠标事件返回 `true`

#### Scenario: imgui 不需要捕获时放行事件

- **WHEN** imgui 报告 want_capture_mouse 为 false
- **THEN** `on_input` 对鼠标事件返回 `false`，App 可将事件传给 camera 等

### Requirement: GuiPlugin 在 init 中注册 font texture

`GuiPlugin::init` SHALL 构建 imgui font atlas 并通过 `PluginInitCtx` 中的 `RenderWorld` / `BindlessManager` 注册 font texture 为 bindless 资源。

#### Scenario: Font texture 注册到 bindless

- **WHEN** `GuiPlugin::init(ctx)` 被调用
- **THEN** imgui font atlas 被构建
- **AND** font texture 被上传到 GPU 并注册到 `BindlessManager`

### Requirement: GuiPlugin 提供 contribute_passes 方法

`GuiPlugin` SHALL 提供 `prepare_render_data(&mut self, ctx: &PluginRenderCtx)` 方法上传当前 frame 的 GUI mesh，并提供 `contribute_passes(&self, graph: &mut RenderGraphBuilder, ctx: &PluginRenderCtx)` 方法，将 GUI overlay 作为 render graph pass 注入。`contribute_passes` SHALL 假设当前 frame 的 mesh 已由 `prepare_render_data` 准备完成。

#### Scenario: GUI pass 添加到 render graph

- **WHEN** App 在 render hook 中调用 `gui.contribute_passes(&mut graph, ctx)`
- **THEN** 一个读写 canvas color attachment 的 GUI draw pass 被添加到 graph
- **AND** 如果 draw data 的 total_vtx_count 为 0，pass SHALL skip 绘制

#### Scenario: GUI mesh 在 render hook 中准备

- **WHEN** App 进入 `FrameAppHooks::render(&mut self, ctx)`
- **THEN** App SHALL 先调用 `gui.prepare_render_data(ctx)` 上传当前 frame 的 vertex/index buffer
- **AND** 再调用 `gui.contribute_passes(&mut graph, ctx)` 注入 GUI pass

#### Scenario: App 不再需要手动创建 GuiRgPass

- **WHEN** App 使用 `GuiPlugin::contribute_passes`
- **THEN** 不需要手动构造 `GuiRgPass` struct 并传递 `gui_draw_data`、`gui_mesh`、`tex_map` 等字段

### Requirement: GuiPlugin 管理 GPU mesh buffer

`GuiPlugin` SHALL 持有 per-frame GUI mesh buffer（现在 `GuiBackend` 的 `gui_meshes`）和 texture map（`tex_map`），负责每帧的 `prepare_render_data` 上传。

#### Scenario: GUI mesh 数据每帧上传

- **WHEN** `gui.prepare_render_data(ctx)` 被调用
- **THEN** 当前帧的 imgui draw data 被上传到对应 frame-in-flight slot 的 GPU buffer
- **AND** 当前 frame-in-flight slot SHALL 通过 `ctx.render_world.frame_counter.frame_label()` 获取

### Requirement: GuiBackend 从 RenderPresent 剥离

`RenderPresent` SHALL NOT 持有 `GuiBackend` 字段。`RenderBackend` SHALL NOT 提供 `submit_gui_data` 或 `register_gui_font` 方法。GUI 的 GPU 资源管理职责 SHALL 完全由 `GuiPlugin` 承担。

#### Scenario: RenderPresent 无 GuiBackend 字段

- **WHEN** 检查 `RenderPresent` 的字段
- **THEN** SHALL NOT 存在 `gui_backend: GuiBackend` 或任何 GUI 相关字段

#### Scenario: RenderBackend 无 GUI 方法

- **WHEN** 检查 `RenderBackend` 的公开方法
- **THEN** SHALL NOT 存在 `submit_gui_data` 或 `register_gui_font` 方法

#### Scenario: GuiPlugin 自行管理 GuiBackend

- **WHEN** `GuiPlugin::init` 被调用
- **THEN** GuiPlugin 利用 `PluginInitCtx` 中的 `RenderWorld`（`BindlessManager`、`GfxResourceManager`）创建 GUI GPU 资源
- **AND** 后续的 mesh 上传和 font 注册由 `GuiPlugin` 自行完成，不经过 `RenderBackend`

### Requirement: GuiPlugin 位于上层集成边界

`GuiPlugin` SHALL 位于 `truvis-gui-plugin` 或等价上层集成 crate。该 crate 可依赖 `truvis-gui-backend`、`truvis-render-graph` 和 `truvis-frame-api`。`truvis-gui-backend` SHALL 继续只提供底层 GUI mesh/pass 录制能力，不依赖 render graph 或 frame runtime。

#### Scenario: gui-backend 不反向依赖上层

- **WHEN** 检查 `truvis-gui-backend` 依赖
- **THEN** SHALL NOT 引入 `truvis-render-graph`、`truvis-frame-runtime` 或 demo app crate

### Requirement: 不使用 GuiPlugin 的 App 无 GUI 开销

不引入 `GuiPlugin` 的 App SHALL 不承担 GUI 相关的 CPU 或 GPU 开销。`PluginRenderCtx` 和 `PluginUpdateCtx` SHALL NOT 包含 imgui 相关字段。

#### Scenario: 无 GUI App 的 Ctx 纯净

- **WHEN** App 不持有 `GuiPlugin`
- **THEN** `PluginRenderCtx` 中 SHALL NOT 存在 `gui_draw_data` 字段
- **AND** 无 imgui 相关内存分配或 GPU buffer 创建
