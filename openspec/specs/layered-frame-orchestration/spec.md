## ADDED Requirements

### Requirement: FrameRuntime drives Renderer lifecycle without field access

FrameRuntime SHALL interact with Renderer exclusively through its public lifecycle methods and returned Ctx types. Direct access to Renderer's internal fields (world, render_world, timer, render_present, fif_timeline_semaphore, cmd_allocator) SHALL be eliminated.

#### Scenario: No direct field access in run_frame
- **WHEN** FrameRuntime executes its frame loop
- **THEN** all Renderer state is accessed only via Renderer's lifecycle methods or the Ctx structs they return

#### Scenario: Sibling field access during Ctx lifetime
- **WHEN** RendererUpdateCtx is alive (Renderer is borrowed)
- **THEN** FrameRuntime SHALL still access its own fields (plugin, gui_host, camera_controller, input_manager, overlays)

### Requirement: FrameRuntime composes RenderCtx from Renderer Ctx and own data

FrameRuntime SHALL construct the Plugin-facing `RenderCtx` by combining `RendererRenderCtx` fields (render_world, render_present, timeline) with FrameRuntime-owned data (gui_draw_data from gui_host). The composition SHALL happen within a block scope where RendererRenderCtx is alive.

#### Scenario: RenderCtx composition
- **WHEN** FrameRuntime enters render phase
- **THEN** it obtains `RendererRenderCtx` from `renderer.render_phase()`, adds `gui_host.get_render_data()`, and constructs `RenderCtx` for the plugin

#### Scenario: gui_draw_data comes from FrameRuntime, not Renderer
- **WHEN** Plugin receives `RenderCtx`
- **THEN** the gui_draw_data field originates from FrameRuntime's gui_host, not from Renderer's internal state

### Requirement: FrameRuntime run_frame follows strict phase ordering

FrameRuntime's `run_frame()` SHALL call Renderer lifecycle methods in this order:
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
- **THEN** all Renderer lifecycle methods and FrameRuntime phases execute in the specified order without interleaving

#### Scenario: Ctx drop gates subsequent methods
- **WHEN** update_phase Ctx has not been dropped
- **THEN** submit_gui_data and prepare are not callable (compile-time enforcement via borrow)

### Requirement: Resize is driven by FrameRuntime with Renderer producing conditional Ctx

FrameRuntime SHALL call `renderer.handle_resize(new_size)`. If the method returns `Some(ctx)`, FrameRuntime SHALL pass the Ctx to `plugin.on_resize()`. If it returns `None`, no plugin notification occurs.

#### Scenario: Plugin notified on actual resize
- **WHEN** `renderer.handle_resize(size)` returns `Some(RendererResizeCtx)`
- **THEN** FrameRuntime calls `plugin.on_resize(ctx)` with the returned Ctx

#### Scenario: Plugin not notified when no resize
- **WHEN** `renderer.handle_resize(size)` returns `None`
- **THEN** FrameRuntime does not call `plugin.on_resize()`

### Requirement: Plugin init receives camera separately from Renderer Ctx

Plugin's `init` hook SHALL receive `RendererInitCtx` (from Renderer) and `&mut Camera` (from FrameRuntime) as separate parameters. This reflects that camera belongs to FrameRuntime, not Renderer.

#### Scenario: Plugin init signature
- **WHEN** Plugin's init is called
- **THEN** it receives `(&mut RendererInitCtx, &mut Camera)` as distinct parameters

### Requirement: Three-layer lifecycle independence

Each layer (Renderer, FrameRuntime, App/Plugin) SHALL only know about its own lifecycle and the Ctx it produces or consumes. Specifically:
- Renderer SHALL NOT reference AppPlugin, FrameRuntime, or any Plugin-related type
- FrameRuntime SHALL NOT reference specific application logic (demo-specific code)
- Plugin SHALL NOT reference Renderer directly (only through Ctx types)

#### Scenario: Renderer has no plugin dependency
- **WHEN** inspecting Renderer's source and dependencies
- **THEN** there are no imports or references to AppPlugin, FrameRuntime, overlays, or gui_host

#### Scenario: Adding a new plugin hook does not modify Renderer
- **WHEN** a new plugin lifecycle hook is needed (e.g., tick)
- **THEN** only FrameRuntime and the plugin trait require modification; Renderer remains unchanged (assuming existing Ctx is sufficient)
