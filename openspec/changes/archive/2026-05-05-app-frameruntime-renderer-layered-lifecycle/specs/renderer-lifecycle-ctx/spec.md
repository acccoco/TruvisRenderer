## ADDED Requirements

### Requirement: Renderer update_phase returns RendererUpdateCtx

Renderer SHALL provide a `pub fn update_phase(&mut self) -> RendererUpdateCtx<'_>` method that returns a typed context containing mutable access to World, mutable access to PipelineSettings, read access to FrameSettings, read access to AccumData, swapchain extent, and delta time. The method SHALL perform internal preparation (update_frame_settings, acquire_image) before returning the Ctx. The returned Ctx SHALL borrow Renderer fields, preventing other Renderer method calls until dropped.

#### Scenario: FrameRuntime obtains update context
- **WHEN** FrameRuntime calls `renderer.update_phase()`
- **THEN** a `RendererUpdateCtx` is returned with valid borrows to world, pipeline_settings, frame_settings, accum_data, swapchain_extent, and delta_time

#### Scenario: Renderer is locked while Ctx alive
- **WHEN** `RendererUpdateCtx` has not been dropped
- **THEN** any call to another `&mut self` method on Renderer SHALL fail at compile time

#### Scenario: Renderer is usable after Ctx drop
- **WHEN** `RendererUpdateCtx` is dropped (block scope ends)
- **THEN** subsequent Renderer methods (submit_gui_data, prepare, etc.) are callable

### Requirement: Renderer render_phase returns RendererRenderCtx with shared borrow

Renderer SHALL provide a `pub fn render_phase(&self) -> RendererRenderCtx<'_>` method that returns a typed context containing shared (read-only) access to RenderWorld, RenderPresent, and the fif timeline semaphore. The method signature SHALL use `&self` (not `&mut self`) to express that render phase does not modify Renderer state.

#### Scenario: FrameRuntime obtains render context
- **WHEN** FrameRuntime calls `renderer.render_phase()`
- **THEN** a `RendererRenderCtx` is returned with read-only borrows to render_world, render_present, and timeline

#### Scenario: Render context does not contain gui_draw_data
- **WHEN** `RendererRenderCtx` is constructed
- **THEN** it SHALL NOT contain gui_draw_data (that field is provided by FrameRuntime separately)

### Requirement: Renderer submit_gui_data accepts external draw data

Renderer SHALL provide a `pub fn submit_gui_data(&mut self, draw_data: &imgui::DrawData)` method that uploads GUI vertex/index data to GPU buffers. The method SHALL NOT know or care where the draw data originates.

#### Scenario: GUI data uploaded after UI compilation
- **WHEN** FrameRuntime calls `renderer.submit_gui_data(draw_data)` after update_phase Ctx is dropped and gui_host has compiled UI
- **THEN** Renderer uploads the draw data to GPU buffers via gui_backend.prepare_render_data

#### Scenario: submit_gui_data is callable between update_phase and prepare
- **WHEN** update_phase Ctx has been dropped AND prepare has not been called
- **THEN** submit_gui_data SHALL succeed

### Requirement: Renderer handle_resize conditionally produces RendererResizeCtx

Renderer SHALL provide a `pub fn handle_resize(&mut self, new_size: [u32; 2]) -> Option<RendererResizeCtx<'_>>` method. It SHALL return `Some(ctx)` only when swapchain was actually rebuilt, and `None` otherwise.

#### Scenario: Size change triggers swapchain rebuild
- **WHEN** new_size differs from current swapchain extent AND surface capabilities confirm resize needed
- **THEN** Renderer rebuilds swapchain and returns `Some(RendererResizeCtx)` with `&mut RenderWorld` and `&RenderPresent`

#### Scenario: No-op when size unchanged
- **WHEN** new_size matches current swapchain extent
- **THEN** returns `None` without rebuilding

### Requirement: Renderer init produces RendererInitCtx

Renderer SHALL provide an init lifecycle method that returns `RendererInitCtx` containing `&mut World`, `&mut RenderWorld`, `&mut CmdAllocator`, `GfxSwapchainImageInfo`, and `&RenderPresent`. The Ctx SHALL NOT contain camera (which belongs to FrameRuntime).

#### Scenario: FrameRuntime initializes after window creation
- **WHEN** FrameRuntime calls renderer init after window/surface creation
- **THEN** a `RendererInitCtx` is returned without camera reference

#### Scenario: Camera passed separately during init
- **WHEN** Plugin's init hook is called
- **THEN** camera is provided as a separate parameter by FrameRuntime, not via RendererInitCtx

### Requirement: Renderer begin_frame and end_frame are self-contained

`begin_frame()` and `end_frame()` SHALL be `&mut self` methods that perform all internal bookkeeping (timer tick, FIF wait, resource cleanup, bindless begin_frame, asset update for begin; frame counter advance for end) without returning any Ctx or requiring external input.

#### Scenario: begin_frame performs internal lifecycle
- **WHEN** FrameRuntime calls `renderer.begin_frame()`
- **THEN** timer ticks, FIF timeline wait completes, commands reset, resources cleaned, bindless manager advances, assets updated

#### Scenario: end_frame advances frame
- **WHEN** FrameRuntime calls `renderer.end_frame()`
- **THEN** frame counter advances to next frame

### Requirement: Renderer prepare accepts camera as direct parameter

`prepare(&mut self, camera: &Camera)` SHALL accept camera state as a direct function parameter. It SHALL perform accum frame update, GPU scene upload, and per-frame descriptor update internally.

#### Scenario: GPU data prepared with external camera
- **WHEN** FrameRuntime calls `renderer.prepare(camera)`
- **THEN** GPU scene is uploaded and descriptors updated using the provided camera's view/projection matrices
