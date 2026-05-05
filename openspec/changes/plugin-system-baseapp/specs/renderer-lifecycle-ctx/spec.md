## MODIFIED Requirements

### Requirement: Renderer update_phase returns RendererUpdateCtx

Renderer SHALL continue to provide `pub fn update_phase(&mut self) -> RendererUpdateCtx<'_>` and return mutable access to update-stage data. The returned Ctx SHALL borrow Renderer fields, preventing other Renderer lifecycle calls until dropped. After this change, `RendererUpdateCtx` is consumed by `BaseApp` and `FrameAppHooks`, not by `FrameRuntime`.

#### Scenario: BaseApp obtains update context

- **WHEN** BaseApp calls `renderer.update_phase()`
- **THEN** a `RendererUpdateCtx` is returned with update-stage borrows
- **AND** App may construct `PluginUpdateCtx` from this Renderer Ctx

#### Scenario: Update context drop releases Renderer borrow

- **WHEN** `RendererUpdateCtx` goes out of scope
- **THEN** subsequent Renderer methods (`prepare`, `render_phase`, etc.) are callable

### Requirement: Renderer render_phase returns RendererRenderCtx with shared borrow

Renderer SHALL continue to provide `pub fn render_phase(&self) -> RendererRenderCtx<'_>` with shared access to `RenderWorld`, `RenderPresent`, and the FIF timeline semaphore. `RendererRenderCtx` SHALL NOT contain `gui_draw_data`; GUI draw data and GUI mesh state belong to `GuiPlugin`.

#### Scenario: BaseApp obtains render context

- **WHEN** BaseApp calls `renderer.render_phase()`
- **THEN** a `RendererRenderCtx` is returned with read-only borrows to render_world, render_present, and timeline

#### Scenario: Render context does not contain GUI data

- **WHEN** `RendererRenderCtx` is constructed
- **THEN** it SHALL NOT contain `gui_draw_data` or imgui-specific fields
- **AND** App may construct `PluginRenderCtx` from this Renderer Ctx

### Requirement: Renderer init produces RendererInitCtx

Renderer SHALL continue to provide an init lifecycle method that returns `RendererInitCtx` containing `&mut World`, `&mut RenderWorld`, `&mut CmdAllocator`, `GfxSwapchainImageInfo`, and `&RenderPresent`. The Ctx SHALL NOT contain camera; camera belongs to App.

#### Scenario: BaseApp initializes after window creation

- **WHEN** BaseApp calls renderer init after window/surface creation
- **THEN** Renderer creates surface/swapchain and returns `RendererInitCtx`
- **AND** App may construct `PluginInitCtx` from this Renderer Ctx

#### Scenario: Camera is not part of RendererInitCtx

- **WHEN** App initializes Plugin instances
- **THEN** camera is provided by App-owned state, not by `RendererInitCtx`

### Requirement: Renderer begin_frame and end_frame are self-contained

`begin_frame()` and `end_frame()` SHALL remain Renderer lifecycle methods for Renderer-owned bookkeeping. This change SHALL NOT move `World`, `AssetHub`, or `RenderWorld` ownership out of Renderer.

#### Scenario: begin_frame performs internal lifecycle

- **WHEN** BaseApp calls `renderer.begin_frame()`
- **THEN** Renderer performs its current internal frame lifecycle work without depending on App, Plugin, or GuiPlugin types

### Requirement: Renderer prepare accepts camera as direct parameter

`prepare(&mut self, camera: &Camera)` SHALL continue to accept camera state as a direct function parameter. After this change, BaseApp obtains the camera through `FrameAppHooks::camera()` instead of owning camera state itself.

#### Scenario: BaseApp prepares with App camera

- **WHEN** BaseApp reaches prepare phase
- **THEN** BaseApp calls `renderer.prepare(app.camera())`
- **AND** Renderer does not own or update camera input state

## REMOVED Requirements

### Requirement: Renderer submit_gui_data accepts external draw data

**Reason**: GUI data upload moves from Renderer into `GuiPlugin`; Renderer no longer owns `GuiBackend` and should not expose GUI-specific data injection methods.
**Migration**: `GuiPlugin::prepare_render_data(&mut self, &PluginRenderCtx)` uploads imgui draw data into its own per-frame mesh buffers before `GuiPlugin::contribute_passes`.
