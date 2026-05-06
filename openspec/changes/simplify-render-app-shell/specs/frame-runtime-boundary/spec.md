## ADDED Requirements

### Requirement: 帧编排类型语义明确为 RenderAppShell

主框架中的共享帧编排实现 SHALL 以 `RenderAppShell` 语义暴露（位于 `truvis-frame-runtime`），并承担 `RenderBackend` 生命周期、输入事件队列和固定帧阶段调度职责。

#### Scenario: 帧编排入口为 RenderAppShell

- **WHEN** 调用方需要把具体 app state 交给渲染循环
- **THEN** SHALL 通过 `truvis_frame_runtime::RenderAppShell` 包装具体 app hooks
- **AND** `truvis_frame_runtime::BaseApp` SHALL NOT 作为 public API 暴露

#### Scenario: RenderAppShell 持有帧基础设施

- **WHEN** `RenderAppShell` 完成窗口绑定初始化
- **THEN** 它 SHALL 持有 `RenderBackend` 和待处理 `InputEvent` 队列
- **AND** 具体 app SHALL NOT 直接持有或调用 `BaseApp`

### Requirement: RenderAppShell SHALL 采用显式 phase 编排

`RenderAppShell` SHALL 以显式阶段组织每帧执行流程，至少包含 input/update/prepare/render/present，并保持阶段顺序稳定。

#### Scenario: 阶段顺序稳定

- **WHEN** `RenderAppShell` 推进任意一帧
- **THEN** input、update、prepare、render、present 阶段 SHALL 按固定顺序执行
- **AND** 每个阶段在同一帧内 SHALL 至多执行一次

#### Scenario: prepare 阶段由 shell 持有

- **WHEN** `RenderAppShell` 进入 prepare 阶段
- **THEN** 它 SHALL 通过 `RenderAppHooks::camera()` 获取相机引用并调用 `RenderBackend::prepare`
- **AND** `RenderAppHooks` SHALL NOT 暴露独立 prepare hook

### Requirement: RenderAppHooks SHALL 作为具体 App 的单一 hook 契约

具体 app state SHALL 实现单一 `RenderAppHooks` trait，以接收 `RenderAppShell` 的生命周期和每帧回调。

#### Scenario: 单一 hooks trait 覆盖生命周期和每帧回调

- **WHEN** 具体 app 接入 `RenderAppShell`
- **THEN** 它 SHALL 实现 `RenderAppHooks`
- **AND** `RenderAppHooks` SHALL 覆盖 init、on_input、update、render、camera、on_resize、shutdown 语义

#### Scenario: 不再拆分 FrameAppState 与 FrameAppHooks

- **WHEN** 检查具体 demo app 的 runtime trait impl
- **THEN** 每个 demo SHALL 使用单一 `RenderAppHooks` impl 表达 shell 回调点
- **AND** SHALL NOT 同时实现 `FrameAppState` 和 `FrameAppHooks`

### Requirement: RenderAppShell SHALL 保持 App 特定状态外置

`RenderAppShell` SHALL NOT 持有 GUI、CameraController、Overlay、InputState 或具体 render pipeline plugin；这些状态 SHALL 由实现 `RenderAppHooks` 的具体 app 持有并编排。

#### Scenario: 添加新 render plugin 不修改 shell

- **WHEN** 新增或替换具体 render pipeline plugin
- **THEN** 变更 SHALL 局限于具体 app 或 plugin 实现
- **AND** `RenderAppShell` SHALL 不需要了解该 plugin 的具体类型

## REMOVED Requirements

### Requirement: 帧编排类型语义明确为 FrameRuntime

**Reason**: The runtime model no longer exposes `FrameRuntime`; current code uses shell-based app adaptation and this change renames the final public shell to `RenderAppShell`.

**Migration**: Use `truvis_frame_runtime::RenderAppShell` as the shared frame orchestration type.

### Requirement: 应用扩展点升级为 FramePlugin（单 trait 多 hook）

**Reason**: Demo-level orchestration is now expressed through concrete app hooks plus reusable `Plugin` instances, not a monolithic `FramePlugin`.

**Migration**: Implement `RenderAppHooks` on the concrete app state and use `Plugin` only for reusable app-owned capability units.

### Requirement: FrameRuntime SHALL 采用显式 phase 编排

**Reason**: The explicit phase skeleton remains required, but it is now owned by `RenderAppShell` rather than `FrameRuntime`.

**Migration**: Validate phase ordering through `RenderAppShell` and `RenderAppHooks`.
