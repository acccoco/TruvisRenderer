## 1. Gfx Root Owner And Typed Ctx

- [x] 1.1 Add explicit `Gfx::new(app_name, instance_extra_exts) -> Gfx` construction while temporarily preserving old singleton APIs for migration.
- [x] 1.2 Add typed borrowed Ctx types for device, resource(device + allocator), queue, surface/swapchain, device-info(properties/limits/format support), and immediate-command capabilities.
- [x] 1.3 Add `Gfx` accessor methods that produce the typed Ctx views without exposing mutable global state or full `&Gfx` to narrow consumers.
- [x] 1.4 Add explicit `Gfx` root-owner destroy/wait-idle methods that do not depend on `G_GFX`.
- [x] 1.5 Update `truvis-gfx` module docs to describe Ctx purpose and the temporary migration compatibility boundary.

## 2. truvis-gfx Explicit Resource Lifecycle

- [x] 2.1 Convert `GfxBuffer` creation, transfer, flush, clear, and destroy paths to receive typed resource/device/queue Ctx instead of calling `Gfx::get`.
- [x] 2.2 Convert `GfxImage` and `GfxImageView` creation and destroy paths to receive typed Ctx and keep Drop diagnostic-only.
- [x] 2.3 Convert command pool and command buffer allocation, reset, free, recording, debug labels, and command methods to explicit device/queue/immediate Ctx or explicit owner references.
- [x] 2.4 Convert fence and semaphore creation, wait, and destroy paths to explicit device Ctx.
- [x] 2.5 Convert descriptor layout, descriptor set, descriptor pool, and descriptor write paths to explicit device Ctx.
- [x] 2.6 Convert shader module, shader module cache, pipeline layout, graphics pipeline, sampler, and query pool paths to explicit device Ctx.
- [x] 2.7 Convert acceleration structure build, query, address, and destroy paths to explicit resource/device/queue Ctx.
- [x] 2.8 Convert surface and swapchain creation, query, acquire, present, rebuild, and destroy paths to explicit surface/swapchain Ctx, including `GfxSurface` explicit destroy before `Window` drop.
- [x] 2.9 Convert `Gfx::one_time_exec`, wait-idle helpers, debug naming/labels, queue-family accessors, device limits/properties, RT properties, and format support queries to typed Ctx rather than global access.
- [x] 2.10 Ensure all `truvis-gfx` Vulkan/VMA/WSI wrapper `Drop` impls are diagnostic-only and no Drop calls Vulkan/VMA/WSI release APIs.

## 3. render-interface Owner Migration

- [x] 3.1 Update `CmdAllocator` to receive typed Gfx Ctx during construction, reset, free, and destroy.
- [x] 3.2 Update `GfxResourceManager` release, cleanup, delayed destroy, and shutdown paths to pass typed Ctx into image/view destruction.
- [x] 3.3 Update `GlobalDescriptorSets` construction/destruction and descriptor set access helpers for explicit device Ctx.
- [x] 3.4 Update `BindlessManager` and `RenderSamplerManager` descriptor write and sampler setup paths for explicit device Ctx.
- [x] 3.5 Update `FifBuffers`, `GpuScene`, stage buffer helpers, and per-frame buffer creation/destruction to use explicit resource/device/queue Ctx.
- [x] 3.6 Preserve `RenderWorld` as a lifetime-free plain owner; do not store typed Gfx Ctx references inside RenderWorld resources.

## 4. RenderBackend And Runtime Ctx Migration

- [x] 4.1 Change `RenderBackend` to own `Gfx` directly and construct it through `Gfx::new`.
- [x] 4.2 Update `RenderBackend::new` initialization order to create all World/RenderWorld resources through typed Gfx Ctx.
- [x] 4.3 Add typed Gfx Ctx fields or accessors to `RenderBackendInitCtx`, `RenderBackendRenderCtx`, `RenderBackendResizeCtx`, and `RenderBackendShutdownCtx`, including device-info, immediate-command, queue, and surface/swapchain capabilities where phase-appropriate.
- [x] 4.4 Update `PluginInitCtx`, `PluginRenderCtx`, `PluginResizeCtx`, and `PluginShutdownCtx` to carry only phase-appropriate typed Gfx Ctx.
- [x] 4.5 Update `RenderBackend::begin_frame`, `prepare`, `render_phase`, `handle_resize`, `shutdown_phase`, and `destroy` to use owned Gfx Ctx instead of global access.
- [x] 4.6 Ensure `RenderBackend::destroy` releases app/plugin resources, present resources, render-world resources, managers, command allocator, sync objects, and descriptor resources before destroying `Gfx`.

## 5. Upper Layer Call Site Migration

- [x] 5.1 Update `RenderPresent` and present/swapchain resize/destroy paths to receive typed Gfx Ctx from RenderBackend and explicitly destroy swapchain and surface resources.
- [x] 5.2 Update `truvis-render-graph` and compute/render pass execution paths to use Ctx provided by render-phase contexts.
- [x] 5.3 Update `truvis-render-passes` pipeline and ray tracing setup/recording paths to use typed Gfx Ctx.
- [x] 5.4 Update `truvis-asset` upload, texture creation, and AssetHub cleanup paths to use typed Gfx Ctx.
- [x] 5.5 Update `truvis-gui-backend` and `GuiPlugin` font/mesh/pipeline/resource cleanup paths to use typed Gfx Ctx.
- [x] 5.6 Update demo apps and render pipeline plugins to submit queues and destroy resources through explicit Ctx.

## 6. Remove Compatibility APIs

- [x] 6.1 Delete `G_GFX` and remove `Gfx::get`, `Gfx::init`, and singleton `Gfx::destroy` APIs.
- [x] 6.2 Run `rg "Gfx::get\\(|Gfx::init\\(|Gfx::destroy\\(|G_GFX" engine/crates` and eliminate all production Rust matches.
- [x] 6.3 Audit `impl Drop for` in `truvis-gfx` and confirm no Vulkan/VMA/WSI release call remains in Drop, including `destroy_surface`, `destroy_swapchain`, VMA destroy, descriptor/pipeline/sampler/query/sync release calls.
- [x] 6.4 Audit resource structs to confirm long-lived wrappers outside `Gfx` root owner do not store typed Gfx Ctx references.
- [x] 6.5 Audit helper/API usage for `one_time_exec`, `wait_idel`, `gfx_queue_family`, `compute_queue_family`, `transfer_queue_family`, `min_ubo_offset_align`, `rt_pipeline_props`, `find_supported_format`, debug naming, and debug labels; ensure each path receives an explicit typed Ctx.

## 7. Documentation And Verification

- [x] 7.1 Update `ARCHITECTURE.md` resource lifecycle section from RAII-owned/manager-owned split to explicit owner-owned destroy rules.
- [x] 7.2 Update `engine/crates/truvis-gfx/README.md` to document `Gfx` root owner, typed Gfx Ctx, explicit destroy, and Drop diagnostics.
- [x] 7.3 Update `engine/crates/truvis-render-interface/README.md` and `engine/crates/truvis-render-backend/README.md` for Ctx fields and shutdown responsibilities.
- [x] 7.4 Run repository formatting command from `justfile`. No format target exists; ran `cargo fmt --all`.
- [x] 7.5 Run repository Rust compile/check command from `justfile`.
- [x] 7.6 Run at least one lightweight demo smoke test when local Vulkan/runtime environment permits. Not run: available demo targets open long-running interactive Vulkan windows in this session.
- [x] 7.7 Run `openspec status --change remove-global-gfx-context` and confirm the change remains apply-ready.
