## MODIFIED Requirements

### Requirement: 帧编排类型语义明确为 BaseApp

主框架中的帧编排入口 SHALL 以 `BaseApp` 语义暴露（位于 `truvis-frame-runtime`），承担不变的帧骨架执行职责。`FrameRuntime` 被 `BaseApp` 替代。

#### Scenario: 帧编排入口为 BaseApp

- **WHEN** 调用方需要帧编排基础设施
- **THEN** SHALL 通过 `truvis_frame_runtime::BaseApp` 访问
- **AND** `BaseApp` SHALL NOT 持有 Plugin、Camera、GUI context 等 app 特定状态

### Requirement: BaseApp SHALL 采用 hook 回调编排取代直接 plugin 调用

`BaseApp` SHALL 通过 `FrameAppHooks` trait 在帧骨架的变化点回调 App，而非直接调用 `FramePlugin` 的 hook。

#### Scenario: 阶段顺序稳定

- **WHEN** 运行任意一帧
- **THEN** begin_frame / on_input / update / prepare / render / present / end_frame 阶段 SHALL 按固定顺序执行

#### Scenario: 每帧阶段执行次数可预测

- **WHEN** 单帧渲染流程被执行
- **THEN** 每个 phase SHALL 在该帧内至多执行一次

### Requirement: BaseApp SHALL 不包含 GUI 编排逻辑

`BaseApp` SHALL NOT 包含 imgui context 管理、GUI font 注册、GUI data submit 或 GUI compile 等逻辑。这些职责 SHALL 由 `GuiPlugin` 承担，由 App 在 hook 中编排。

#### Scenario: BaseApp 源码无 imgui 依赖

- **WHEN** 检查 `BaseApp` 的源码和 crate 依赖
- **THEN** SHALL NOT 存在对 `imgui`、`GuiHost`、`GuiBackend` 的引用

#### Scenario: GUI 编排由 App hook 完成

- **WHEN** 需要在帧中进行 GUI 编排
- **THEN** 相关调用 SHALL 出现在 App 的 `FrameAppHooks::update` 和 `FrameAppHooks::render` 实现中

### Requirement: Renderer lifecycle Ctx SHALL 保持 App 裁剪入口

`Renderer` SHALL 继续通过 lifecycle methods 产出 `RendererInitCtx` / `RendererUpdateCtx` / `RendererRenderCtx` / `RendererResizeCtx`。本 change SHALL NOT 迁移 `World`、`AssetHub` 或 `RenderWorld` 的所有权；App 通过 Renderer Ctx 裁剪出 Plugin Ctx。

#### Scenario: Renderer Ctx 仍是 Plugin Ctx 的来源

- **WHEN** `FrameAppHooks::update` 或 `FrameAppHooks::render` 被调用
- **THEN** App SHALL 从对应 Renderer Ctx 构造 Plugin 层 Ctx
- **AND** Renderer SHALL NOT 依赖 `FrameApp`、`Plugin`、`GuiPlugin` 或任何 App 具体类型

### Requirement: Camera SHALL 由 App 持有

Camera 和 CameraController SHALL 由 App 持有，而非 BaseApp。BaseApp 通过 `FrameAppHooks::camera()` 在 prepare 阶段获取 camera 引用。

#### Scenario: BaseApp 不持有 Camera

- **WHEN** 检查 `BaseApp` 的字段
- **THEN** SHALL NOT 包含 `Camera` 或 `CameraController` 字段

#### Scenario: prepare 阶段通过 hook 获取 camera

- **WHEN** BaseApp 执行 prepare 阶段
- **THEN** SHALL 调用 `app.camera()` 获取 camera 引用传给 `renderer.prepare(camera)`

## REMOVED Requirements

### Requirement: 应用扩展点升级为 FramePlugin（单 trait 多 hook）

**Reason**: `FramePlugin` 被 `FrameApp` + `FrameAppHooks` + `Plugin` 三个 trait 替代。App 不再是被动的 plugin，而是主动的编排者。
**Migration**: 实现 `FrameApp` + `FrameAppHooks` trait，将原 `FramePlugin` hook 逻辑移入 App 的 hook 实现中。原 Plugin 逻辑拆分为独立的 `Plugin` 实现。

### Requirement: 默认 overlay SHALL 可注册而非硬编码

**Reason**: `OverlayModule` 概念被合并入 `Plugin` 体系。Overlay 成为实现 `Plugin` trait 的普通 Plugin，由 App 自行持有。
**Migration**: 将 `DebugInfoOverlay` 和 `PipelineControlsOverlay` 改为实现 `Plugin` trait 的 struct。对于需要 camera / swapchain_extent / accum_frames 等额外数据的 overlay，暴露特有方法（如 `build_overlay_ui(ui, camera, extent, accum_frames)`），由 App 在 GUI 帧内调用并传入 App 持有的数据。
