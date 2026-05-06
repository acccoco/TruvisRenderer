## Context

The current renderer has several lifecycle styles for Vulkan/VMA objects:

- RAII wrappers whose `Drop` performs the actual Vulkan/VMA destruction, such as `GfxBuffer`, samplers, pipelines, and acceleration structures.
- Explicitly destroyed wrappers where `destroy_mut(&mut self)` performs destruction and `Drop` only asserts that destruction already happened, such as `GfxImage`, `GfxImageView`, `GfxResourceManager`, and `FifBuffers`.
- Cloneable handle wrappers with manual destruction, such as `GfxSemaphore`, where multiple Rust values can refer to the same raw Vulkan handle.

These styles are all individually understandable, but similar resource types use different contracts. The most problematic path is shutdown: app-owned plugins hold GPU objects while `RenderAppShell` destroys `RenderBackend` and `Gfx`. If those app/plugin objects are later dropped, their `Drop` implementations may call `Gfx::get()` after `Gfx::destroy()`. VMA errors are also hard to diagnose because allocation creation does not consistently attach stable project resource names to VMA allocation user data.

## Goals / Non-Goals

**Goals:**

- Define a small, explicit lifecycle taxonomy for GPU resources.
- Ensure app/plugin-owned GPU objects are released before backend and `Gfx` teardown.
- Make VMA allocation diagnostics include stable project resource names from `GfxBuffer::new` and `GfxImage::new`.
- Reduce ambiguous APIs where a type exposes both `Drop` and multiple explicit destroy forms.
- Preserve existing frame, resize, and render-thread ownership semantics.
- Update architecture and crate documentation as part of the implementation.

**Non-Goals:**

- No RenderGraph redesign.
- No broad migration away from the current `Gfx` singleton.
- No new external dependency for GPU lifetime tracking.
- No attempt to make Vulkan resources usable across threads.
- No full resource leak detector beyond focused debug assertions and logging.

## Decisions

### D1: Use two lifecycle categories

**Choice**: Every GPU-owning wrapper is classified as either RAII-owned or manager-owned.

RAII-owned resources destroy their Vulkan/VMA handle in `Drop`. They may keep a `destroy(self)` method only as a drop-now alias, but they must not expose `destroy_mut(&mut self)` because in-place destruction leaves a partially valid Rust value.

Manager-owned resources are destroyed through a manager or lifecycle owner. Their `Drop` must not call Vulkan/VMA destruction directly; it only asserts or logs that explicit release happened. Delayed destruction, immediate destruction, and dependency ordering stay in the manager.

**Rationale**: The distinction matches the current code's real split while making it explicit. It avoids pretending all Vulkan resources can use one model: frame-delayed images need manager ownership, while short-lived buffers and pipeline objects can remain RAII-owned.

**Alternatives considered**:

- Make every resource RAII. Rejected because frame-delayed image destruction and view dependency cleanup already require manager coordination.
- Make every resource manager-owned. Rejected because it would over-expand `GfxResourceManager` and make simple local resources heavier.

### D2: Plugin shutdown must release app-owned GPU resources

**Choice**: App-owned plugins that allocate GPU resources must release them in `Plugin::shutdown` or app shutdown before `RenderBackend::destroy()` and `Gfx::destroy()`.

`Plugin::shutdown` must receive a typed shutdown context, `PluginShutdownCtx` or an equivalent API, with access to the backend-owned managers needed for release. At minimum this context must expose mutable `RenderWorld` access so plugins can unregister bindless references and remove manager-owned resources through `GfxResourceManager` while the backend is still alive. If app hooks own GPU resources outside the standard plugin visitor, `RenderAppHooks::shutdown` must receive an equivalent typed app shutdown context before plugin shutdown runs.

Plugins with `Option<T>` GPU fields should `take()` those fields during shutdown so their RAII `Drop` runs while `Gfx` is still alive. Resources registered in `GfxResourceManager` must unregister bindless references and remove manager-owned images/views through the manager while the backend context is available. App-owned command buffer handle clones should be cleared during shutdown; the backing command pools remain owned and destroyed by `CmdAllocator`.

**Rationale**: This fixes the highest-risk boundary without moving concrete render plugins back into `RenderBackend` or `RenderAppShell`.

**Alternatives considered**:

- Rely on app drop order after shell shutdown. Rejected because app fields can drop after `Gfx::destroy()`.
- Let `RenderAppShell` know concrete plugin fields and drop them. Rejected because it violates the current App Hooks / Shell / Backend separation.

### D3: Introduce a VMA allocation debug helper

**Choice**: Add a project helper around `vk_mem::AllocationCreateInfo` that attaches VMA user data with `USER_DATA_COPY_STRING`.

The helper should accept a resource debug name and a base allocation create info, build a temporary C string, set `user_data`, add `USER_DATA_COPY_STRING`, and run the VMA create call while the pointer is valid. `GfxBuffer::new` and `GfxImage::new` should use this helper instead of setting allocation metadata ad hoc at call sites.

**Rationale**: VMA copies the string when `USER_DATA_COPY_STRING` is set, so the helper can keep raw pointer handling local and safe. Centralizing the pattern prevents future allocation call sites from forgetting debug metadata.

**Alternatives considered**:

- Require every caller to set `user_data`. Rejected because raw pointer lifetime handling is easy to get wrong and creates repeated boilerplate.
- Only log names in Rust. Rejected because VMA-side diagnostics still lack allocation identity.

### D4: Prefer API removal or renaming over compatibility shims

**Choice**: Ambiguous lifecycle APIs should be removed or renamed during this change, even if that causes workspace-local call sites to change.

Examples:

- `destroy_mut` should be reserved for explicitly documented manager-owned state that can be safely reset in place, or removed.
- Empty `destroy(self)` methods should be removed unless they document a meaningful drop-now alias.
- Cloneable Vulkan owner wrappers should be split into an owner plus raw-handle access instead of relying on `Clone + destroy(self)`.

**Rationale**: The bug source is semantic ambiguity, not only missing calls. Keeping old names as wrappers would preserve the same mental overhead.

**Alternatives considered**:

- Keep all old methods and only add documentation. Rejected because call sites would still look equivalent while doing different things.

### D5: Add lifecycle logging at destruction boundaries

**Choice**: Destruction paths should log resource name, raw handle, manager handle when available, and destroy reason at debug level or higher for unusual cases.

Destroy reasons should be represented by a small project enum or equivalent typed value, for example `Shutdown`, `Resize`, `DeferredCleanup`, and `ImmediateRelease`. Manager release APIs should accept or derive this reason and pass it to the concrete destruction path. The goal is not verbose per-frame logging, but enough identity when a VMA or validation error appears.

**Rationale**: VMA user data helps allocator diagnostics; Rust-side logs help correlate which manager path or shutdown stage triggered destruction.

## Risks / Trade-offs

**[Wide call-site churn]** -> Keep the first implementation focused on `GfxBuffer`, `GfxImage`, `GfxImageView`, `GfxResourceManager`, plugin shutdown, and semaphore ownership. Broader cleanup can follow once the contract is stable.

**[Spec scope vs. first implementation]** -> The requirement is intentionally project-wide, but the first implementation should inventory every GPU-owning wrapper and explicitly mark any wrapper deferred to follow-up work. A wrapper may remain outside the first code cleanup only if its current lifecycle contract is documented and it is not on the shutdown bug path.

**[Shutdown context availability]** -> Plugin shutdown needs backend-owned manager access for GUI font texture and bindless cleanup, so this change should extend shutdown with a typed context rather than reaching into `RenderBackend`.

**[Pending asset uploads]** -> `AssetUploadManager` may hold staging buffers, command buffers, and not-yet-registered images while uploads are in flight. Shutdown must wait or drain those uploads before manager and `Gfx` teardown, otherwise pending RAII resources can drop after their allocator/device is gone.

**[RAII Drop still depends on Gfx singleton]** -> The change does not eliminate this dependency. It enforces that RAII drops occur before `Gfx::destroy()` and documents that invariant.

**[VMA helper unsafe internals]** -> Keep unsafe pointer handling inside one small helper and test it through `GfxBuffer::new` / `GfxImage::new` allocation paths.

**[Over-logging]** -> Use debug-level logging for normal destruction and reserve warn/error for violated lifecycle assertions or invalid handles.

## Lifecycle Inventory

### First implementation scope

| Area | Wrapper / owner | Contract | Current conflict and resolution |
| --- | --- | --- | --- |
| `truvis-gfx` buffers | `GfxBuffer`, special buffer wrappers, SBT / structured / stage / vertex / index / acceleration buffers | RAII-owned | Keep `Drop` as owner release; `destroy(self)` is documented as a drop-now alias. |
| `truvis-gfx` images | `GfxImage` | Manager-owned or lifecycle-owner-owned | Removed public `destroy_mut`; explicit release is `destroy(reason)`, and `Drop` asserts release happened. |
| `truvis-gfx` image views | `GfxImageView` | Manager-owned | Removed public `destroy_mut`; release is through `GfxResourceManager` before image release. |
| `truvis-gfx` semaphores | `GfxSemaphore` | Unique explicit owner | Removed `Clone`; submit code uses references/raw handles, and exactly one owner calls `destroy(self)`. |
| `truvis-render-interface` manager | `GfxResourceManager` | Manager-owned root | Delayed cleanup, immediate release, resize, and shutdown now pass a typed `DestroyReason`. |
| `truvis-render-interface` FIF resources | `FifBuffers` | Manager-owned handles | `destroy_mut` unregisters bindless entries and releases images through `GfxResourceManager`; views follow images. |
| `truvis-render-interface` GPU scene | `GpuScene` | Mixed: RAII buffers plus manager-owned built-in textures | Shutdown now unregisters built-in texture SRVs, releases their images through the manager, and drops TLAS while `Gfx` is alive. |
| `truvis-render-interface` commands | `CmdAllocator`, `GfxCommandPool`, `GfxCommandBuffer` | Allocator/pool-owned; command buffer handles are non-owning clones | `CmdAllocator::destroy()` frees tracked command buffers and destroys pools; app/plugin command buffer clones are cleared during shutdown. |
| `truvis-asset` textures | `AssetHub`, `AssetUploadManager` | Manager/lifecycle-owner-owned | Shutdown drains pending uploads, frees command buffers, destroys pending images, unregisters ready textures, and releases all ready images. |
| `truvis-app` plugins | `GuiPlugin`, `RtPipeline`, `TrianglePlugin`, `ShaderToyPlugin` | App-owned GPU resources released during plugin shutdown | Shutdown contexts now expose mutable `RenderWorld`; plugins `take()` RAII GPU fields before backend teardown. |
| `truvis-frame-runtime` shell | `RenderAppShell` | Shutdown coordinator | App and plugin shutdown now run before `RenderBackend::destroy()` and `Gfx::destroy()`. |

### Deferred wrappers

| Wrapper / owner | Current contract | Deferred reason |
| --- | --- | --- |
| Foundation roots: `Gfx`, `GfxCore`, `GfxDevice`, `GfxInstance`, `GfxPhysicalDevice`, `GfxDebugMsger`, `VMemAllocator` | Backend/root RAII or explicit root teardown | They are owned by `Gfx::destroy()` / backend root teardown, not app/plugin fields on the shutdown bug path. |
| Swapchain roots: `GfxSurface`, `GfxSwapchain` | Backend/present-owned explicit teardown | They are released through `RenderPresent::destroy()` or resize rebuild before `Gfx::destroy()`. |
| Descriptor/pipeline/query/shader helpers | RAII-owned or explicit temporary owner | Empty drop-now aliases were made explicit where touched; remaining manual shader module destruction is a focused temporary creation contract. |
| Experimental scene-side `MaterialManager` / `MeshManager` | RAII-owned GPU buffers/BLAS behind manager values | Not currently wired into `RenderBackend` shutdown path used by the app demos; follow-up can integrate them with the same shutdown context if they become runtime-owned. |

### Verification notes

- Remaining `destroy_mut` names are documented manager/reset paths: `FifBuffers::destroy_mut` unregisters bindless and releases manager-owned images, `GpuScene::destroy_mut` unregisters built-in textures and clears manager handles, and `CmdAllocator::destroy_mut` is private implementation behind `CmdAllocator::destroy()`.
- `SceneManager::destroy_mut` is CPU-side state cleanup and not a GPU wrapper lifecycle contract.
- Remaining `Drop` implementations that call `Gfx::get()` are RAII-owned wrappers (`GfxBuffer`, sampler, query pool, acceleration, descriptor/pipeline resources, compute/RT pipeline internals, swapchain/surface roots). Runtime app/plugin owners now `take()` these fields during shutdown, and backend-owned roots are destroyed before `Gfx::destroy()`.
- Manager-owned wrappers on the shutdown bug path (`GfxImage`, `GfxImageView`, `GfxResourceManager`, `FifBuffers`, `GpuScene`, `AssetUploadManager`, `CmdAllocator`, `GfxSemaphore`) now assert on missed explicit release instead of calling `Gfx::get()` from `Drop`.

## Migration Plan

1. Document lifecycle categories and classify current GPU-owning wrappers.
2. Add the VMA allocation debug helper and migrate `GfxBuffer::new` / `GfxImage::new`.
3. Add typed shutdown contexts and fix app/plugin shutdown so app-owned GPU objects are dropped or manager-released before backend/Gfx teardown.
4. Drain `AssetHub` and `AssetUploadManager` shutdown paths, including ready textures and in-flight uploads.
5. Normalize `destroy`, `destroy_mut`, and `Drop` APIs for the VMA resource path.
6. Address `GfxSemaphore` ownership by removing clone-based owner semantics or replacing clone usage with handle/reference passing.
7. Add resource destruction logging and focused debug assertions.
8. Update `ARCHITECTURE.md` and crate READMEs.
9. Run formatting, `cargo check`, and at least one render smoke test from `justfile`.

## Open Questions

- Should buffer resources remain RAII-owned long term, or should buffers registered in `GfxResourceManager` become manager-owned wrappers distinct from local `GfxBuffer`?
