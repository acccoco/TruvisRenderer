## MODIFIED Requirements

### Requirement: App SHALL 通过 Plugin 和 FrameAppHooks 访问运行时能力

App（通过 `FrameAppHooks`）SHALL 在各 hook 中接收 Renderer 的 typed Ctx，并自主裁剪后传给 Plugin。Plugin SHALL 通过 `PluginInitCtx` / `PluginUpdateCtx` / `PluginRenderCtx` / `PluginResizeCtx` 访问能力。

#### Scenario: App hook 接收 Renderer Ctx

- **WHEN** `FrameAppHooks::update` 被 BaseApp 调用
- **THEN** App 接收 `&mut RendererUpdateCtx`，可从中构造 `PluginUpdateCtx` 传给 Plugin

#### Scenario: Plugin 不直接接触 Renderer Ctx

- **WHEN** Plugin 需要读取帧状态或提交渲染命令
- **THEN** SHALL 通过 Plugin 层 Ctx（`PluginInitCtx` / `PluginUpdateCtx` / `PluginRenderCtx`）完成
- **AND** Plugin 代码 SHALL NOT 依赖 `RendererUpdateCtx` 或 `RendererRenderCtx` 的具体字段布局

### Requirement: BaseApp SHALL 作为帧编排单入口

`BaseApp` SHALL 作为渲染线程中的帧骨架入口，render_loop 通过 `Box<dyn FrameApp>` 驱动帧推进。`FrameApp::run_frame` 内部通过 `BaseApp::run_frame` 执行骨架。

#### Scenario: render loop 通过 FrameApp 驱动

- **WHEN** 渲染线程主循环推进单帧
- **THEN** 调度逻辑 SHALL 通过 `FrameApp::run_frame()` 完成
- **AND** render_loop SHALL NOT 直接访问 Renderer 或 BaseApp

#### Scenario: winit 入口表达 App 而非 Plugin

- **WHEN** 外部启动 demo app
- **THEN** SHALL 使用 `WinitApp::run_app(|| Box<dyn FrameApp>)` 或等价 App factory 入口
- **AND** 新入口命名 SHALL NOT 暗示传入的是单一 `FramePlugin`

#### Scenario: 帧节流决策点唯一

- **WHEN** 系统判定是否推进下一帧
- **THEN** render_loop SHALL 通过 `FrameApp::time_to_render()` 查询
- **AND** App SHALL 委托给 `BaseApp::time_to_render()`（内部调 `Renderer::time_to_render()`）
- **AND** 节流判断 SHALL 仅在此单一链路执行

### Requirement: prepare 阶段职责 SHALL 由 BaseApp 帧骨架持有

prepare 阶段的调度与执行 SHALL 由 `BaseApp::run_frame` 的帧骨架固定执行。Plugin 不引入独立 prepare hook。

#### Scenario: Plugin 契约不包含 prepare hook

- **WHEN** 定义 `Plugin` 生命周期 hook
- **THEN** 契约 SHALL 包含 `init / on_input / update / on_resize / shutdown`
- **AND** SHALL NOT 暴露独立 `prepare` hook

#### Scenario: prepare 在 render 前由 BaseApp 骨架完成

- **WHEN** BaseApp 执行帧骨架
- **THEN** SHALL 在 `app.render()` 前调用 `renderer.prepare(app.camera())`

### Requirement: Demo SHALL 迁移到 FrameApp + Plugin 架构

四个官方 demo SHALL 迁移为实现 `FrameApp` + `FrameAppHooks` 的 App struct，内部持有具体 Plugin。

#### Scenario: 四 demo 完成迁移

- **WHEN** 运行 `triangle` / `rt-cornell` / `rt-sponza` / `shader-toy`
- **THEN** 四者均 SHALL 实现 `FrameApp` + `FrameAppHooks` trait
- **AND** 各 app 内部 SHALL 持有 `GuiPlugin` 和对应的渲染 Plugin

### Requirement: crate 边界 SHALL 反映新架构

- `truvis-frame-api`：SHALL 定义 `Plugin` trait、`FrameApp` trait、`FrameAppHooks` trait 和所有 Plugin Ctx 类型
- `truvis-frame-runtime`：SHALL 定义 `BaseApp` struct 和帧骨架实现

#### Scenario: Plugin trait 和 App trait 在 frame-api 中

- **WHEN** 外部 crate 需要实现 Plugin 或 FrameApp
- **THEN** SHALL 从 `truvis-frame-api` 导入相关 trait 和 Ctx 类型

#### Scenario: BaseApp 在 frame-runtime 中

- **WHEN** App 需要使用帧骨架
- **THEN** SHALL 从 `truvis-frame-runtime` 导入 `BaseApp`

## REMOVED Requirements

### Requirement: FramePlugin SHALL 通过 typed contexts 访问运行时能力

**Reason**: `FramePlugin` 被移除，替代为 `FrameApp` + `FrameAppHooks`（App 层）和 `Plugin`（Plugin 层）。typed contexts 机制保留，但消费者和传递路径变化。
**Migration**: 原 `FramePlugin` 实现拆分为 App（实现 `FrameApp` + `FrameAppHooks`）和若干 Plugin（实现 `Plugin` trait）。App 在 hook 中从 Renderer Ctx 裁剪出 Plugin Ctx 传给 Plugin。

### Requirement: FrameRuntime SHALL 成为帧编排单入口

**Reason**: `FrameRuntime` 被 `BaseApp` 替代。帧编排单入口语义保留，载体变化。
**Migration**: 将 `FrameRuntime` 重构为 `BaseApp`，移除 Plugin/GUI/Camera/Overlay 持有，保留 Renderer + 输入事件队列 + 帧骨架。
