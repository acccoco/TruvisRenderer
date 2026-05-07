## MODIFIED Requirements

### Requirement: RenderBackend update_phase returns RenderBackendUpdateCtx

RenderBackend SHALL continue to provide `pub fn update_phase(&mut self) -> RenderBackendUpdateCtx<'_>` and return mutable access to update-stage data. The returned Ctx SHALL borrow RenderBackend fields, preventing other RenderBackend lifecycle calls until dropped. After this change, `RenderBackendUpdateCtx` is consumed by `BaseApp` and `FrameAppHooks`, not by `FrameRuntime`.

#### Scenario: BaseApp obtains update context

- **WHEN** BaseApp calls `render_backend.update_phase()`
- **THEN** a `RenderBackendUpdateCtx` is returned with update-stage borrows
- **AND** App may construct `PluginUpdateCtx` from this RenderBackend Ctx

#### Scenario: Update context drop releases RenderBackend borrow

- **WHEN** `RenderBackendUpdateCtx` goes out of scope
- **THEN** subsequent RenderBackend methods (`prepare`, `render_phase`, etc.) are callable

### Requirement: RenderBackend render_phase returns RenderBackendRenderCtx with shared borrow

RenderBackend SHALL continue to provide `pub fn render_phase(&self) -> RenderBackendRenderCtx<'_>` with shared access to `RenderWorld`, `RenderPresent`, and the FIF timeline semaphore. `RenderBackendRenderCtx` SHALL NOT contain `gui_draw_data`; GUI draw data and GUI mesh state belong to `GuiPlugin`.

#### Scenario: BaseApp obtains render context

- **WHEN** BaseApp calls `render_backend.render_phase()`
- **THEN** a `RenderBackendRenderCtx` is returned with read-only borrows to render_world, render_present, and timeline

#### Scenario: Render context does not contain GUI data

- **WHEN** `RenderBackendRenderCtx` is constructed
- **THEN** it SHALL NOT contain `gui_draw_data` or imgui-specific fields
- **AND** App may construct `PluginRenderCtx` from this RenderBackend Ctx

### Requirement: RenderBackend init produces RenderBackendInitCtx

RenderBackend SHALL continue to provide an init lifecycle method that returns `RenderBackendInitCtx` containing `&mut World`, `&mut RenderWorld`, `&mut CmdAllocator`, `GfxSwapchainImageInfo`, and `&RenderPresent`. The Ctx SHALL NOT contain camera; camera belongs to App.

#### Scenario: BaseApp initializes after window creation

- **WHEN** BaseApp calls render_backend init after window/surface creation
- **THEN** RenderBackend creates surface/swapchain and returns `RenderBackendInitCtx`
- **AND** App may construct `PluginInitCtx` from this RenderBackend Ctx

#### Scenario: Camera is not part of RenderBackendInitCtx

- **WHEN** App initializes Plugin instances
- **THEN** camera is provided by App-owned state, not by `RenderBackendInitCtx`

### Requirement: RenderBackend begin_frame and end_frame are self-contained

`begin_frame()` and `end_frame()` SHALL remain RenderBackend lifecycle methods for RenderBackend-owned bookkeeping. This change SHALL NOT move `World`, `AssetHub`, or `RenderWorld` ownership out of RenderBackend.

#### Scenario: begin_frame performs internal lifecycle

- **WHEN** BaseApp calls `render_backend.begin_frame()`
- **THEN** RenderBackend performs its current internal frame lifecycle work without depending on App, Plugin, or GuiPlugin types

### Requirement: RenderBackend prepare accepts camera as direct parameter

`prepare(&mut self, camera: &Camera)` SHALL continue to accept camera state as a direct function parameter. After this change, BaseApp obtains the camera through `FrameAppHooks::camera()` instead of owning camera state itself.

#### Scenario: BaseApp prepares with App camera

- **WHEN** BaseApp reaches prepare phase
- **THEN** BaseApp calls `render_backend.prepare(app.camera())`
- **AND** RenderBackend does not own or update camera input state

## REMOVED Requirements

### Requirement: RenderBackend submit_gui_data accepts external draw data

**Reason**: GUI data upload moves from RenderBackend into `GuiPlugin`; RenderBackend no longer owns `GuiBackend` and should not expose GUI-specific data injection methods.
**Migration**: `GuiPlugin::prepare_render_data(&mut self, &PluginRenderCtx)` uploads imgui draw data into its own per-frame mesh buffers before `GuiPlugin::contribute_passes`.
