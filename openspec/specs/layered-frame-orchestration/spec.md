## ADDED Requirements

### Requirement: FrameRuntime drives RenderBackend lifecycle without field access

FrameRuntime SHALL interact with RenderBackend exclusively through its public lifecycle methods and returned Ctx types. Direct access to RenderBackend's internal fields (world, render_world, timer, render_present, fif_timeline_semaphore, cmd_allocator) SHALL be eliminated.

#### Scenario: No direct field access in run_frame
- **WHEN** FrameRuntime executes its frame loop
- **THEN** all RenderBackend state is accessed only via RenderBackend's lifecycle methods or the Ctx structs they return

#### Scenario: Sibling field access during Ctx lifetime
- **WHEN** RenderBackendUpdateCtx is alive (RenderBackend is borrowed)
- **THEN** FrameRuntime SHALL still access its own fields (plugin, gui_host, camera_controller, input_manager, overlays)

### Requirement: FrameRuntime composes RenderCtx from RenderBackend Ctx and own data

FrameRuntime SHALL construct the Plugin-facing `RenderCtx` by combining `RenderBackendRenderCtx` fields (render_world, render_present, timeline) with FrameRuntime-owned data (gui_draw_data from gui_host). The composition SHALL happen within a block scope where RenderBackendRenderCtx is alive.

#### Scenario: RenderCtx composition
- **WHEN** FrameRuntime enters render phase
- **THEN** it obtains `RenderBackendRenderCtx` from `render_backend.render_phase()`, adds `gui_host.get_render_data()`, and constructs `RenderCtx` for the plugin

#### Scenario: gui_draw_data comes from FrameRuntime, not RenderBackend
- **WHEN** Plugin receives `RenderCtx`
- **THEN** the gui_draw_data field originates from FrameRuntime's gui_host, not from RenderBackend's internal state

### Requirement: FrameRuntime run_frame follows strict phase ordering

FrameRuntime's `run_frame()` SHALL call RenderBackend lifecycle methods in this order:
1. `begin_frame()`
2. [FrameRuntime own: process_input]
3. `update_phase()` → Ctx used for build_ui + plugin.update → Ctx dropped
4. [FrameRuntime own: compile_ui]
5. `submit_gui_data(draw_data)`
6. [FrameRuntime own: update_camera]
7. `prepare(camera)`
8. `render_phase()` → compose RenderCtx → plugin.render → Ctx dropped
9. `present()`
10. `end_frame()`

#### Scenario: Full frame execution order
- **WHEN** `run_frame()` is called
- **THEN** all RenderBackend lifecycle methods and FrameRuntime phases execute in the specified order without interleaving

#### Scenario: Ctx drop gates subsequent methods
- **WHEN** update_phase Ctx has not been dropped
- **THEN** submit_gui_data and prepare are not callable (compile-time enforcement via borrow)

### Requirement: Resize is driven by FrameRuntime with RenderBackend producing conditional Ctx

FrameRuntime SHALL call `render_backend.handle_resize(new_size)`. If the method returns `Some(ctx)`, FrameRuntime SHALL pass the Ctx to `plugin.on_resize()`. If it returns `None`, no plugin notification occurs.

#### Scenario: Plugin notified on actual resize
- **WHEN** `render_backend.handle_resize(size)` returns `Some(RenderBackendResizeCtx)`
- **THEN** FrameRuntime calls `plugin.on_resize(ctx)` with the returned Ctx

#### Scenario: Plugin not notified when no resize
- **WHEN** `render_backend.handle_resize(size)` returns `None`
- **THEN** FrameRuntime does not call `plugin.on_resize()`

### Requirement: Plugin init receives camera separately from RenderBackend Ctx

Plugin's `init` hook SHALL receive `RenderBackendInitCtx` (from RenderBackend) and `&mut Camera` (from FrameRuntime) as separate parameters. This reflects that camera belongs to FrameRuntime, not RenderBackend.

#### Scenario: Plugin init signature
- **WHEN** Plugin's init is called
- **THEN** it receives `(&mut RenderBackendInitCtx, &mut Camera)` as distinct parameters

### Requirement: Three-layer lifecycle independence

Each layer (RenderBackend, FrameRuntime, App/Plugin) SHALL only know about its own lifecycle and the Ctx it produces or consumes. Specifically:
- RenderBackend SHALL NOT reference FramePlugin, FrameRuntime, or any Plugin-related type
- FrameRuntime SHALL NOT reference specific application logic (demo-specific code)
- Plugin SHALL NOT reference RenderBackend directly (only through Ctx types)

#### Scenario: RenderBackend has no plugin dependency
- **WHEN** inspecting RenderBackend's source and dependencies
- **THEN** there are no imports or references to FramePlugin, FrameRuntime, overlays, or gui_host

#### Scenario: Adding a new plugin hook does not modify RenderBackend
- **WHEN** a new plugin lifecycle hook is needed (e.g., tick)
- **THEN** only FrameRuntime and the plugin trait require modification; RenderBackend remains unchanged (assuming existing Ctx is sufficient)
