## ADDED Requirements

### Requirement: 三层模型 SHALL 使用 App Hooks / RenderAppShell / RenderBackend

主渲染编排 SHALL 使用 App Hooks / RenderAppShell / RenderBackend 三层模型。具体 app hooks 持有 app 特定状态和 plugin，`RenderAppShell` 持有共享帧骨架与 backend 基础设施，`RenderBackend` 持有 GPU/backend 生命周期能力。

#### Scenario: 分层职责清晰

- **WHEN** 检查主渲染编排依赖方向
- **THEN** `RenderAppShell` SHALL 调用 `RenderBackend` public lifecycle methods
- **AND** `RenderAppShell` SHALL 通过 `RenderAppHooks` 回调具体 app
- **AND** `RenderBackend` SHALL NOT 引用 `RenderApp`、`RenderAppShell`、`RenderAppHooks`、GUI、overlay 或具体 render pipeline plugin

#### Scenario: BaseApp 不再作为独立层

- **WHEN** 描述最终 runtime layer model
- **THEN** SHALL NOT 将 `BaseApp` 描述为 App 与 RenderBackend 之间的 public layer
- **AND** 其原有职责 SHALL 归属 `RenderAppShell`

### Requirement: RenderAppShell SHALL drive RenderBackend lifecycle without field access

`RenderAppShell` SHALL interact with `RenderBackend` exclusively through public lifecycle methods and returned Ctx types. Direct access to `RenderBackend` internal fields SHALL remain forbidden.

#### Scenario: No direct field access in run_frame

- **WHEN** `RenderAppShell` executes its frame loop
- **THEN** all `RenderBackend` state SHALL be accessed only via `RenderBackend` lifecycle methods or the Ctx structs they return

#### Scenario: App access occurs through hook contexts

- **WHEN** `RenderAppShell` receives a backend Ctx
- **THEN** it SHALL pass the controlled Ctx or shell-level wrapper Ctx to `RenderAppHooks`
- **AND** concrete app code SHALL use that Ctx to construct any narrower `Plugin*Ctx`

### Requirement: RenderAppShell run_frame SHALL follow strict phase ordering

`RenderAppShell::run_frame()` SHALL call `RenderBackend` lifecycle methods and app hooks in this order:

1. `RenderBackend::begin_frame()`
2. drain shell-owned input event queue and call `RenderAppHooks::on_input(events)`
3. `RenderBackend::update_phase()` then `RenderAppHooks::update(ctx)`
4. `RenderBackend::prepare(app.camera())`
5. `RenderBackend::render_phase()` then `RenderAppHooks::render(ctx)`
6. `RenderBackend::present()`
7. `RenderBackend::end_frame()`

#### Scenario: Full frame execution order

- **WHEN** `RenderAppShell::run_frame()` is called
- **THEN** all backend lifecycle methods and app hooks SHALL execute in the specified order without interleaving

#### Scenario: Ctx lifetime gates subsequent phases

- **WHEN** an update or render Ctx is alive
- **THEN** later backend lifecycle methods that require a conflicting borrow SHALL NOT be callable until that Ctx is dropped

### Requirement: Resize SHALL be driven through RenderAppShell

Resize SHALL be driven through `RenderAppShell`. The shell SHALL call `RenderBackend::handle_resize(new_size)`. If the method returns `Some(ctx)`, the shell SHALL wrap it as `RenderAppResizeCtx` and call `RenderAppHooks::on_resize(ctx)`.

#### Scenario: App notified on actual resize

- **WHEN** `RenderBackend::handle_resize(size)` returns `Some(RenderBackendResizeCtx)`
- **THEN** `RenderAppShell` SHALL call `RenderAppHooks::on_resize(RenderAppResizeCtx)`

#### Scenario: App not notified when no resize

- **WHEN** `RenderBackend::handle_resize(size)` returns `None`
- **THEN** `RenderAppShell` SHALL NOT call `RenderAppHooks::on_resize`

### Requirement: Three-layer lifecycle independence

Each layer SHALL only know about its own lifecycle and the Ctx it produces or consumes.

#### Scenario: RenderBackend has no app dependency

- **WHEN** inspecting `RenderBackend` source and dependencies
- **THEN** there SHALL be no imports or references to `RenderApp`, `RenderAppShell`, `RenderAppHooks`, GUI plugin, overlays, or demo-specific pipeline types

#### Scenario: Shell has no app-specific plugin dependency

- **WHEN** inspecting `RenderAppShell` source and dependencies
- **THEN** there SHALL be no imports or references to concrete app plugins, overlays, GUI implementation details, camera controller, or demo-specific pipeline types

#### Scenario: Adding a new plugin type does not modify backend or shell

- **WHEN** a new app-owned plugin type is added
- **THEN** only concrete app/plugin code SHALL need modification
- **AND** `RenderBackend` and `RenderAppShell` SHALL remain unchanged unless new backend Ctx capability is required
