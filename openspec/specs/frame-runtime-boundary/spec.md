## Purpose

定义主框架帧编排语义（`FrameRuntime`）、应用扩展契约（`AppPlugin`）、显式 phase 编排与 `Renderer` 后端职责边界，以及默认 overlay 注册方式与旧 API 兼容收口条件，使 runtime 与 GPU backend 分工可被契约化验证，并与 `render-threading` 既有线程与关闭语义保持兼容。

## Requirements

### Requirement: 帧编排类型语义明确为 FrameRuntime

主框架中的帧编排入口 SHALL 以 `FrameRuntime` 语义暴露（位于 `truvis-frame-runtime`），并承担阶段调度职责。

#### Scenario: 帧编排入口唯一

- **WHEN** 调用方引入应用帧编排入口
- **THEN** SHALL 通过 `truvis_frame_runtime::FrameRuntime` 访问
- **AND** `truvis-app` 可提供 re-export 转发以简化导入路径

### Requirement: 应用扩展点升级为 AppPlugin（单 trait 多 hook）

应用侧扩展契约 SHALL 采用 `AppPlugin` 单 trait 的多阶段 hook 形式（定义在 `truvis-app-api`），每个 hook 接收对应阶段的 typed context。

#### Scenario: 新插件契约覆盖现有生命周期

- **WHEN** 应用实现 `AppPlugin`
- **THEN** SHALL 能表达初始化、更新、UI 构建、渲染相关 hook
- **AND** 框架 SHALL 按既定阶段顺序调用这些 hook

#### Scenario: Hook 顺序定义与实现保持一致

- **WHEN** `FrameRuntime` 文档化每帧内的 `AppPlugin` hook 调用顺序
- **THEN** spec、代码注释与实际调用顺序 SHALL 保持一致
- **AND** 同一 phase 内的子顺序（例如 `build_ui` 与 `update`）SHALL 被明确说明，避免契约歧义
- **AND** 在当前过渡实现中，`phase_update` 内 SHALL 先调用 `build_ui`，再调用 `update`

#### Scenario: 插件契约覆盖 resize 与关闭语义

- **WHEN** 应用需要在窗口尺寸变化后重建依赖 swapchain 的资源
- **THEN** `AppPlugin::on_resize` SHALL 在 swapchain 重建完成后、下一帧渲染提交前被调用
- **AND** `AppPlugin::shutdown` SHALL 在 runtime 销毁前被调用

### Requirement: FrameRuntime SHALL 采用显式 phase 编排

`FrameRuntime` SHALL 以显式阶段函数组织每帧执行流程（至少包含 input/update/prepare/render/present），以替代单体更新函数的隐式边界。

#### Scenario: 阶段顺序稳定

- **WHEN** 运行任意一帧
- **THEN** input/update/prepare/render/present 阶段 SHALL 按固定顺序执行
- **AND** GUI、resize、present 的时序 SHALL 与现有行为保持等价

#### Scenario: 每帧阶段执行次数可预测

- **WHEN** 单帧渲染流程被执行
- **THEN** 每个 phase SHALL 在该帧内至多执行一次
- **AND** resize/out-of-date 重建路径 SHALL 与 phase 编排共享单一入口
- **AND** 线程关闭握手语义 SHALL 与 `render-threading` 既有规范保持兼容

#### Scenario: 重建触发条件覆盖 resize 与 out-of-date

- **WHEN** 渲染线程处理单帧前的 swapchain 重建判定
- **THEN** 在「窗口尺寸变化」或「backend 报告 need_resize（含 out-of-date/suboptimal）」任一条件成立时 SHALL 触发重建
- **AND** 重建 SHALL 仅通过 runtime 的单一入口执行，不得在其他 phase 中引入分叉重建流程
- **AND** 重建成功后 SHALL 在下一次渲染提交前调用 `AppPlugin::on_resize`

### Requirement: Renderer SHALL 收敛为 backend 职责

`Renderer` SHALL 聚焦 GPU backend 能力（device/swapchain/cmd/sync/submit/present），不得继续承载 scene/asset 侧 world 更新调度职责。

#### Scenario: world 更新由 runtime 驱动

- **WHEN** 发生 scene/asset 相关更新推进
- **THEN** 调度入口 SHALL 位于 `FrameRuntime` 的相应 phase
- **AND** `Renderer` SHALL 仅消费已准备好的渲染输入并执行 GPU 提交

### Requirement: Runtime 与 Renderer 的职责边界 SHALL 可被契约化验证

`FrameRuntime` 与 `Renderer` 之间 SHALL 通过稳定的上下文/接口边界协作，避免应用层直接依赖 backend 内部可变实现细节。

#### Scenario: AppPlugin 通过 typed contexts 访问能力

- **WHEN** `AppPlugin` 在各阶段读取或修改渲染相关状态
- **THEN** SHALL 通过对应阶段的 typed context（`InitCtx` / `UpdateCtx` / `RenderCtx` / `ResizeCtx`）完成
- **AND** 不得将 `Renderer` 的内部字段布局视为稳定 API

#### Scenario: Renderer 不再主动推进应用 world 生命周期

- **WHEN** 触发 asset/scene/world 的 CPU 侧更新决策
- **THEN** 决策与调度 SHALL 发生在 runtime/plugin 侧
- **AND** `Renderer` SHALL 仅执行 backend 数据上传与 GPU 执行步骤

### Requirement: 默认 overlay SHALL 可注册而非硬编码

默认调试/信息 overlay SHALL 通过可注册模块接入 runtime，而非固定写在核心编排路径中。

#### Scenario: 可替换默认 overlay

- **WHEN** 应用选择禁用或替换默认 overlay
- **THEN** SHALL 可以在不修改 `FrameRuntime` 核心流程的前提下完成
- **AND** 默认示例应用的用户体验 SHALL 保持不回归

### Requirement: 旧兼容接口已下线

`OuterApp` / `LegacyOuterAppAdapter` / `RenderApp` trait / `WinitApp::run` 已移除。四个 demo 全部通过 `AppPlugin` typed contexts 接入。

#### Scenario: 兼容层已完成收口

- **GIVEN** `triangle`、`rt-cornell`、`rt-sponza`、`shader-toy` 已迁移到 `FrameRuntime` + `AppPlugin`
- **THEN** 旧接口已移除，`truvis-app` 中仅保留 re-export shim 简化导入路径
