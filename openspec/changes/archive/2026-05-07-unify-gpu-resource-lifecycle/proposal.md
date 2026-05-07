## Why

GPU resource teardown currently mixes RAII `Drop`, explicit `destroy(self)`, and in-place `destroy_mut(&mut self)` contracts across similar Vulkan/VMA resources. This makes shutdown, resize, and delayed destruction hard to reason about, and VMA errors do not carry enough resource identity to identify the allocation path quickly.

## What Changes

- Introduce a project-wide GPU resource lifecycle contract with two explicit ownership modes: RAII-owned resources and manager-owned resources.
- Remove ambiguous same-type combinations of `Drop`, `destroy(self)`, and `destroy_mut(&mut self)` unless the contract documents a single clear meaning.
- Ensure app/plugin-owned GPU resources are released during `Plugin::shutdown` or app shutdown before `RenderBackend` and `Gfx` teardown.
- Add typed shutdown context where app/plugin release needs backend-owned manager access.
- Ensure pending GPU uploads are drained or explicitly released before upload command pools, semaphores, managers, and `Gfx` are destroyed.
- Add safe project helpers for VMA allocation debug metadata, wrapping `vk_mem::AllocationCreateInfo::user_data` with `vk_mem::AllocationCreateFlags::USER_DATA_COPY_STRING`.
- Apply the helper in `GfxBuffer::new` and `GfxImage::new` so VMA diagnostics include stable project resource names.
- Add shutdown/debug logging that identifies resource names, handles, manager handles where available, and destroy reason.
- **BREAKING**: Resource lifecycle APIs may be renamed or removed where their current signatures encode ambiguous ownership.

## Capabilities

### New Capabilities

- `gpu-resource-lifecycle`: Defines GPU resource ownership categories, allowed destroy/drop contracts, manager-owned release behavior, and VMA debug metadata requirements.

### Modified Capabilities

- `render-threading`: Tightens shutdown semantics so app/plugin-owned GPU resources are released on the render thread before `RenderBackend` and `Gfx` are destroyed.

## Impact

- Affected crates: `truvis-gfx`, `truvis-render-interface`, `truvis-render-backend`, `truvis-frame-api`, `truvis-frame-runtime`, `truvis-app`, and `truvis-gui-backend`.
- Affected systems: VMA allocation creation, resource manager delayed destruction, plugin shutdown, app shutdown contexts, asset upload shutdown, render backend shutdown, swapchain/resize resource rebuild, and resource lifecycle documentation.
- Documentation updates: `ARCHITECTURE.md` plus relevant crate READMEs for `truvis-gfx`, `truvis-render-interface`, and frame runtime shutdown semantics.
