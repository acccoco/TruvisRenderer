## Purpose

定义 RenderBackend lifecycle methods 产出的 typed Ctx，约束 backend 内部状态借用边界与上层帧编排入口。
## Requirements
### Requirement: RenderBackend update_phase returns RenderBackendUpdateCtx

RenderBackend SHALL continue to provide `pub fn update_phase(&mut self) -> RenderBackendUpdateCtx<'_>` and return mutable access to update-stage data. The returned Ctx SHALL borrow RenderBackend fields, preventing other RenderBackend lifecycle calls until dropped. After this change, `RenderBackendUpdateCtx` is consumed by `BaseApp` and `FrameAppHooks`.

#### Scenario: BaseApp obtains update context

- **WHEN** BaseApp calls `render_backend.update_phase()`
- **THEN** a `RenderBackendUpdateCtx` is returned with update-stage borrows
- **AND** App may construct `PluginUpdateCtx` from this RenderBackend Ctx

#### Scenario: Update context drop releases RenderBackend borrow

- **WHEN** `RenderBackendUpdateCtx` goes out of scope
- **THEN** subsequent RenderBackend methods (`prepare`, `render_phase`, etc.) are callable

### Requirement: RenderBackend handle_resize conditionally produces RenderBackendResizeCtx

RenderBackend SHALL provide a `pub fn handle_resize(&mut self, new_size: [u32; 2]) -> Option<RenderBackendResizeCtx<'_>>` method. It SHALL return `Some(ctx)` only when swapchain was actually rebuilt, and `None` otherwise.

#### Scenario: Size change triggers swapchain rebuild
- **WHEN** new_size differs from current swapchain extent AND surface capabilities confirm resize needed
- **THEN** RenderBackend rebuilds swapchain and returns `Some(RenderBackendResizeCtx)` with `&mut RenderWorld` and `&RenderPresent`

#### Scenario: No-op when size unchanged
- **WHEN** new_size matches current swapchain extent
- **THEN** returns `None` without rebuilding

### Requirement: RenderBackend init produces RenderBackendInitCtx

RenderBackend SHALL continue to provide an init lifecycle method that returns `RenderBackendInitCtx` containing `&mut World`, `&mut RenderWorld`, `&mut CmdAllocator`, `GfxSwapchainImageInfo`, and `&RenderPresent`. The Ctx SHALL NOT contain camera; camera belongs to App.

#### Scenario: BaseApp initializes after window creation

- **WHEN** BaseApp calls render_backend init after window/surface creation
- **THEN** RenderBackend creates surface/swapchain and returns `RenderBackendInitCtx`
- **AND** App may construct `PluginInitCtx` from this RenderBackend Ctx

#### Scenario: Camera is not part of RenderBackendInitCtx

- **WHEN** App initializes Plugin instances
- **THEN** camera is provided by App-owned state, not by `RenderBackendInitCtx`

### Requirement: RenderBackend prepare accepts camera as direct parameter

`prepare(&mut self, camera: &Camera)` SHALL continue to accept camera state as a direct function parameter. After this change, BaseApp obtains the camera through `FrameAppHooks::camera()` instead of owning camera state itself.

#### Scenario: BaseApp prepares with App camera

- **WHEN** BaseApp reaches prepare phase
- **THEN** BaseApp calls `render_backend.prepare(app.camera())`
- **AND** RenderBackend does not own or update camera input state

### Requirement: RenderBackend owns Gfx root owner

`RenderBackend` SHALL directly own the `Gfx` root owner for the render thread. It SHALL construct `Gfx` during backend creation and destroy it only after all child GPU resources have been explicitly released.

#### Scenario: Backend construction creates Gfx
- **WHEN** `RenderBackend::new` is called on the render thread
- **THEN** it SHALL create an owned `Gfx` value
- **AND** it SHALL NOT initialize a global Gfx singleton

#### Scenario: Backend destruction releases Gfx last
- **WHEN** `RenderBackend::destroy` executes
- **THEN** it SHALL explicitly destroy RenderPresent including swapchain and surface WSI handles, RenderWorld resources, World asset GPU resources, command allocator, synchronization objects, descriptor/pipeline resources, and managers before destroying the owned `Gfx`

### Requirement: RenderBackend lifecycle Ctx exposes typed Gfx Ctx where needed

RenderBackend lifecycle contexts SHALL include or otherwise provide typed Gfx Ctx required by their phase consumers. Each phase SHALL expose the narrowest Gfx capability set needed by app/plugin/backend callers.

#### Scenario: Init context supports GPU resource creation
- **WHEN** `RenderBackendInitCtx` is constructed
- **THEN** it SHALL provide typed Gfx Ctx sufficient for plugin/app initialization to create buffers, images, descriptors, pipelines, samplers, command buffers, swapchain-sized resources, debug names, immediate upload commands, and device property queries
- **AND** it SHALL NOT require plugin code to call a global Gfx accessor

#### Scenario: Render context supports command recording and submission setup
- **WHEN** `RenderBackendRenderCtx` is constructed
- **THEN** it SHALL provide typed Gfx Ctx or explicit queue/device references required by render graph pass recording, debug labels, device property access, and queue submission
- **AND** it SHALL keep RenderWorld access read-only

#### Scenario: Resize context supports explicit swapchain resource rebuild
- **WHEN** `RenderBackendResizeCtx` is constructed after a swapchain rebuild
- **THEN** it SHALL provide typed Gfx Ctx sufficient to destroy and recreate swapchain-sized GPU resources and WSI swapchain/surface-dependent resources

#### Scenario: Shutdown context supports explicit GPU cleanup
- **WHEN** `RenderBackendShutdownCtx` is constructed
- **THEN** it SHALL provide typed Gfx Ctx sufficient for app/plugin-owned GPU resources to be explicitly destroyed before backend-owned resources and `Gfx` root owner are destroyed

### Requirement: RenderBackend internal phases do not use global Gfx

RenderBackend internal lifecycle methods SHALL use its owned `Gfx` value and typed Ctx views for all Vulkan/VMA operations. They SHALL NOT call global Gfx singleton APIs.

#### Scenario: Prepare uploads GPU data through explicit context
- **WHEN** `RenderBackend::prepare` uploads GPU scene data, updates per-frame buffers, or writes descriptor sets
- **THEN** it SHALL obtain the required typed Gfx Ctx from its owned `Gfx`
- **AND** it SHALL pass that context to lower-level APIs instead of calling `Gfx::get`

#### Scenario: Resize waits and rebuilds through explicit context
- **WHEN** RenderBackend resizes FIF buffers or RenderPresent rebuilds swapchain resources
- **THEN** all device wait, resource release, and resource creation operations SHALL use explicit typed Gfx Ctx from the backend-owned `Gfx`

### Requirement: Plugin-facing Ctx preserves lifecycle boundaries while carrying Gfx Ctx

Plugin-facing contexts SHALL continue to expose only phase-appropriate World/RenderWorld access while adding the typed Gfx Ctx needed for explicit resource creation and destruction.

#### Scenario: Update context stays CPU-focused
- **WHEN** `PluginUpdateCtx` is constructed
- **THEN** it SHALL continue to expose CPU update state such as `World`, `PipelineSettings`, `FrameSettings`, and delta time
- **AND** it SHALL NOT expose broad GPU resource mutation access unless the update phase has a specific explicit GPU operation

#### Scenario: Shutdown context carries cleanup capability
- **WHEN** `PluginShutdownCtx` is constructed
- **THEN** it SHALL carry typed Gfx Ctx needed to destroy plugin-owned GPU resources
- **AND** plugin shutdown SHALL occur before `RenderBackend::destroy` destroys backend-owned resources and `Gfx`

### Requirement: RenderBackend render_phase returns RenderBackendRenderCtx with shared borrow

RenderBackend SHALL continue to provide `pub fn render_phase(&self) -> RenderBackendRenderCtx<'_>` with shared access to `RenderWorld`, `RenderPresent`, and the FIF timeline semaphore. `RenderBackendRenderCtx` SHALL NOT contain `gui_draw_data`; GUI draw data and GUI mesh state belong to `GuiPlugin`.

#### Scenario: BaseApp obtains render context

- **WHEN** BaseApp calls `render_backend.render_phase()`
- **THEN** a `RenderBackendRenderCtx` is returned with read-only borrows to render_world, render_present, and timeline

#### Scenario: Render context does not contain GUI data

- **WHEN** `RenderBackendRenderCtx` is constructed
- **THEN** it SHALL NOT contain `gui_draw_data` or imgui-specific fields
- **AND** App may construct `PluginRenderCtx` from this RenderBackend Ctx

### Requirement: RenderBackend begin_frame and end_frame are self-contained

`begin_frame()` and `end_frame()` SHALL remain RenderBackend lifecycle methods for RenderBackend-owned bookkeeping. This change SHALL NOT move `World`, `AssetHub`, or `RenderWorld` ownership out of RenderBackend.

#### Scenario: begin_frame performs internal lifecycle

- **WHEN** BaseApp calls `render_backend.begin_frame()`
- **THEN** RenderBackend performs its current internal frame lifecycle work without depending on App, Plugin, or GuiPlugin types
