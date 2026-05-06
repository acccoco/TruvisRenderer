## Why

`BaseApp` was introduced to remove duplicated frame skeleton code, but `FrameAppShell` now already centralizes that skeleton for all demo apps. Keeping both as public concepts adds one extra abstraction layer without adding a distinct ownership boundary in the current architecture.

This change simplifies the render-app API surface while preserving the same frame ordering, plugin orchestration freedom, and render-thread lifecycle semantics.

## What Changes

- **BREAKING**: Rename the render-loop trait from `FrameApp` to `RenderApp`.
- **BREAKING**: Rename `FrameAppShell` to `RenderAppShell`.
- **BREAKING**: Merge the public `BaseApp` responsibilities into `RenderAppShell`; `BaseApp` is removed from the public runtime API.
- **BREAKING**: Merge `FrameAppState` and `FrameAppHooks` into a single app-side hook trait named `RenderAppHooks`.
- Rename shell-provided context types from `FrameAppInitCtx` / `FrameAppResizeCtx` to `RenderAppInitCtx` / `RenderAppResizeCtx`.
- Keep `RenderAppShell` as the single shared implementation of the fixed frame sequence: input, update, prepare, render, present.
- Keep concrete apps responsible for GUI, camera/input state, overlays, and render pipeline plugins through `RenderAppHooks`.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `frame-runtime-boundary`: Replace the public `BaseApp`/`FrameAppShell` split with `RenderAppShell`, and replace `FrameAppHooks`/`FrameAppState` with `RenderAppHooks`.
- `runtime-api-boundary`: Update the render-loop-facing API contract from `FrameApp` to `RenderApp`, and clarify that `RenderApp` is the current runtime trait, not the removed legacy compatibility trait.
- `render-threading`: Update render-thread ownership and shutdown wording to drive `Box<dyn RenderApp>` through `RenderAppShell` without direct `BaseApp` exposure.
- `layered-frame-orchestration`: Update the layer model from App / BaseApp / RenderBackend to App Hooks / RenderAppShell / RenderBackend.

## Impact

- Affected crates: `truvis-frame-api`, `truvis-frame-runtime`, `truvis-winit-app`, `truvis-app`.
- Affected public API: runtime trait names, shell type names, hook trait names, init/resize context type names, and `BaseApp` export removal.
- Affected docs: `README.md`, `ARCHITECTURE.md`, crate READMEs, and OpenSpec specs describing runtime boundaries.
- Expected behavior: no intentional render behavior change; frame order, resize timing, input delivery, plugin shutdown order, and Vulkan resource lifecycle remain equivalent.
