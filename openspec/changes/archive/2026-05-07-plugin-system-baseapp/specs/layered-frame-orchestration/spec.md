## MODIFIED Requirements

### Requirement: FrameRuntime drives RenderBackend lifecycle without field access

The `FrameRuntime` role SHALL be replaced by `BaseApp`. BaseApp SHALL interact with RenderBackend exclusively through public lifecycle methods and returned Ctx types. Direct access to RenderBackend internal fields remains forbidden.

#### Scenario: No direct field access in run_frame

- **WHEN** BaseApp executes its frame loop
- **THEN** all RenderBackend state is accessed only via RenderBackend lifecycle methods or the Ctx structs they return

#### Scenario: RenderBackend Ctx does not block App-owned state

- **WHEN** `RenderBackendUpdateCtx` or `RenderBackendRenderCtx` is alive
- **THEN** App SHALL still be able to access its own fields such as GuiPlugin, CameraController, overlays, and render pipeline plugins

### Requirement: FrameRuntime composes RenderCtx from RenderBackend Ctx and own data

The old `FrameRuntime` composition of `RenderCtx` SHALL be removed. App SHALL receive RenderBackend Ctx through `FrameAppHooks`, then construct Plugin-layer Ctx values for the Plugin instances it owns. GUI draw data SHALL NOT be composed into a shared render context.

#### Scenario: PluginRenderCtx composition

- **WHEN** App enters render hook
- **THEN** it obtains `RenderBackendRenderCtx` from BaseApp
- **AND** it constructs `PluginRenderCtx` containing `render_world`, `render_present`, and `timeline`
- **AND** it does not add `gui_draw_data`

#### Scenario: GUI draw data remains inside GuiPlugin

- **WHEN** GuiPlugin contributes its render graph pass
- **THEN** draw data originates from GuiPlugin-owned imgui context
- **AND** other Plugin Ctx values do not expose imgui draw data

### Requirement: FrameRuntime run_frame follows strict phase ordering

BaseApp SHALL replace `FrameRuntime::run_frame` as the owner of strict frame skeleton ordering:

1. `begin_frame()`
2. drain input events → `app.on_input(events)`
3. `update_phase()` → `app.update(update_ctx)` → Ctx dropped
4. `prepare(app.camera())`
5. `render_phase()` → `app.render(render_ctx)` → Ctx dropped
6. `present()`
7. `end_frame()`

#### Scenario: Strict BaseApp phase ordering

- **WHEN** `BaseApp::run_frame()` is called
- **THEN** all RenderBackend lifecycle methods and App hooks execute in the specified order without interleaving

#### Scenario: GUI upload no longer uses RenderBackend submit_gui_data

- **WHEN** App needs to upload GUI mesh data
- **THEN** App SHALL call `GuiPlugin::prepare_render_data` from render hook
- **AND** BaseApp SHALL NOT call `render_backend.submit_gui_data`

### Requirement: Resize is driven by FrameRuntime with RenderBackend producing conditional Ctx

Resize SHALL be driven through App and BaseApp instead of FrameRuntime. App calls BaseApp resize handling; BaseApp calls `render_backend.handle_resize(new_size)`. If RenderBackend returns `Some(ctx)`, App converts it to `PluginResizeCtx` and notifies relevant Plugin instances.

#### Scenario: Plugin notified on actual resize

- **WHEN** `render_backend.handle_resize(size)` returns `Some(RenderBackendResizeCtx)`
- **THEN** App calls `plugin.on_resize(ctx)` for Plugins that need resize handling

#### Scenario: Plugin not notified when no resize

- **WHEN** `render_backend.handle_resize(size)` returns `None`
- **THEN** App does not call Plugin resize hooks

### Requirement: Three-layer lifecycle independence

Each layer (RenderBackend, BaseApp, App/Plugin) SHALL only know about its own lifecycle and the Ctx it produces or consumes. Specifically:

- RenderBackend SHALL NOT reference `FrameApp`, `Plugin`, `GuiPlugin`, BaseApp, overlays, or app-specific pipeline types
- BaseApp SHALL NOT reference specific application logic, `GuiPlugin`, camera controller, overlays, or render pipeline plugin types
- Plugin SHALL NOT receive RenderBackend directly and SHALL use Plugin Ctx values constructed by App

#### Scenario: RenderBackend has no plugin dependency

- **WHEN** inspecting RenderBackend source and dependencies
- **THEN** there are no imports or references to `FrameApp`, `Plugin`, `GuiPlugin`, overlays, or app-specific pipeline types

#### Scenario: Adding a new Plugin type does not modify RenderBackend or BaseApp

- **WHEN** adding a new render pipeline Plugin
- **THEN** only the App and plugin implementation require modification, assuming existing Ctx values are sufficient

## REMOVED Requirements

### Requirement: Plugin init receives camera separately from RenderBackend Ctx

**Reason**: The old requirement describes `FramePlugin::init(&mut RenderBackendInitCtx, &mut Camera)`. `FramePlugin` is removed, and camera belongs to App.
**Migration**: App initializes camera-owned state directly and passes `PluginInitCtx` only to Plugin lifecycle hooks. Plugins that need camera during initialization should receive it through explicit App-owned concrete methods, not the unified `Plugin` trait.
