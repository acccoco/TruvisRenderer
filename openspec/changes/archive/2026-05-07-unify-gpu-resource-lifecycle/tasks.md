## 1. Lifecycle Inventory

- [x] 1.1 Inventory GPU-owning wrappers in `truvis-gfx`, `truvis-render-interface`, `truvis-render-backend`, `truvis-app`, and `truvis-gui-backend`.
- [x] 1.2 Classify each wrapper as RAII-owned or manager-owned, and note any current `Drop` / `destroy(self)` / `destroy_mut(&mut self)` conflicts.
- [x] 1.3 Mark any GPU-owning wrapper intentionally deferred from first implementation with its current lifecycle contract and reason it is not on the shutdown bug path.

## 2. VMA Debug Metadata

- [x] 2.1 Add a safe project helper that wraps `vk_mem::AllocationCreateInfo::user_data` with `vk_mem::AllocationCreateFlags::USER_DATA_COPY_STRING`.
- [x] 2.2 Use the helper in `GfxBuffer::new` so buffer allocations carry copied VMA debug names.
- [x] 2.3 Use the helper in `GfxImage::new` so image allocations carry copied VMA debug names.
- [x] 2.4 Preserve externally owned image behavior so `GfxImage::from_external` does not attach or imply VMA allocation metadata.

## 3. Shutdown Boundary

- [x] 3.1 Add `PluginShutdownCtx` with mutable `RenderWorld` access, or an equivalent typed shutdown path, so plugins can unregister bindless resources and release manager-owned resources before backend teardown.
- [x] 3.2 Add an equivalent app shutdown context if any `RenderAppHooks` implementation owns GPU resources outside the standard plugin visitor.
- [x] 3.3 Update `RenderAppShell::shutdown` so app hooks and plugin GPU resource release run before `RenderBackend::destroy()` and `Gfx::destroy()`.
- [x] 3.4 Update `GuiPlugin` to release app-owned GPU resources during shutdown, including font texture bindless registration and manager-owned image/view handles.
- [x] 3.5 Update render pipeline plugins (`RtPipeline`, triangle, shader toy) to release owned passes, pipelines, command buffer handle clones, and buffers during shutdown while `Gfx` is alive.
- [x] 3.6 Update `AssetHub::destroy` to unregister and release all ready texture resources, not only the fallback texture.
- [x] 3.7 Update `AssetUploadManager` shutdown to wait for, drain, or explicitly cancel pending uploads so staging buffers, pending images, command buffers, command pool, and timeline semaphore are released while `Gfx` is alive.
- [x] 3.8 Ensure app values can drop after shutdown without any remaining field requiring `Gfx::get()` in `Drop`.

## 4. Resource API Cleanup

- [x] 4.1 Normalize `GfxImage` and `GfxImageView` lifecycle APIs so their explicit destroy path and `Drop` contract are unambiguous.
- [x] 4.2 Normalize `GfxResourceManager` release APIs around delayed cleanup, immediate release, and whole-manager shutdown.
- [x] 4.3 Keep RAII-owned `GfxBuffer` behavior explicit, or split manager-registered buffers from local RAII buffers if inventory shows the current model is unsafe.
- [x] 4.4 Remove or rename empty `destroy(self)` methods that only obscure ownership semantics.
- [x] 4.5 Rework `GfxSemaphore` so cloneable values are not independent owners of the same raw Vulkan semaphore.

## 5. Diagnostics

- [x] 5.1 Add a `DestroyReason` enum or equivalent typed value for shutdown, resize, deferred cleanup, and immediate release paths.
- [x] 5.2 Thread destroy reason through manager release APIs and concrete destruction paths.
- [x] 5.3 Include project debug name, raw Vulkan handle, and manager handle where available in resource destruction logs.
- [x] 5.4 Add debug assertions that catch manager-owned resources dropped without explicit release before `Gfx::destroy()`.

## 6. Documentation

- [x] 6.1 Update `ARCHITECTURE.md` with the GPU resource lifecycle categories and shutdown invariant.
- [x] 6.2 Update `engine/crates/truvis-gfx/README.md` with RAII-owned resource and VMA debug metadata rules.
- [x] 6.3 Update `engine/crates/truvis-render-interface/README.md` with manager-owned resource and delayed destruction rules.
- [x] 6.4 Update `engine/crates/truvis-frame-runtime/README.md` with plugin shutdown ordering and resource-release expectations.

## 7. Verification

- [x] 7.1 Run `cargo fmt`.
- [x] 7.2 Run `cargo check` for the workspace.
- [x] 7.3 Run at least one render smoke test from `justfile` after checking available commands.
- [x] 7.4 Search for remaining same-type `Drop` + `destroy` + `destroy_mut` combinations and confirm each remaining case is documented.
- [x] 7.5 Search for resource `Drop` implementations that call `Gfx::get()` and confirm all such owners are released before `Gfx::destroy()`.
