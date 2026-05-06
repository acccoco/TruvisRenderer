## 1. Runtime API Rename

- [x] 1.1 Rename `FrameApp` to `RenderApp` in `truvis-frame-api`, keeping the object-safe method set unchanged.
- [x] 1.2 Rename all render-loop imports and trait object uses from `FrameApp` to `RenderApp`.
- [x] 1.3 Rename `FrameAppShell` to `RenderAppShell` in `truvis-frame-runtime`.
- [x] 1.4 Rename `FrameAppInitCtx` / `FrameAppResizeCtx` to `RenderAppInitCtx` / `RenderAppResizeCtx`.

## 2. Shell Consolidation

- [x] 2.1 Move `BaseApp` fields (`RenderBackend`, input event queue) into `RenderAppShell`.
- [x] 2.2 Move `BaseApp` lifecycle methods into `RenderAppShell` or private shell helper methods.
- [x] 2.3 Preserve exact frame order: begin_frame, input, update, prepare, render, present, end_frame.
- [x] 2.4 Preserve resize behavior: call `RenderBackend::handle_resize`, notify hooks only when it returns `Some`.
- [x] 2.5 Preserve shutdown order: app hooks shutdown before backend/Gfx destroy.
- [x] 2.6 Remove public `BaseApp` export and delete or privatize obsolete `base_app` module code.

## 3. Hooks Consolidation

- [x] 3.1 Merge `FrameAppState` and `FrameAppHooks` into a single `RenderAppHooks` trait.
- [x] 3.2 Ensure `RenderAppHooks` covers init, on_input, update, render, camera, on_resize, and shutdown.
- [x] 3.3 Migrate `HelloTriangleApp`, `ShaderToy`, `CornellApp`, and `SponzaApp` to one `RenderAppHooks` impl each.
- [x] 3.4 Update plugin ctx construction in app hooks without changing plugin behavior.

## 4. Call Sites And Documentation

- [x] 4.1 Update `truvis-winit-app` entry points to construct `RenderAppShell::new(...)`.
- [x] 4.2 Update crate exports, module docs, and README files for `RenderApp`, `RenderAppShell`, and `RenderAppHooks`.
- [x] 4.3 Update `ARCHITECTURE.md` to describe App Hooks / RenderAppShell / RenderBackend.
- [x] 4.4 Use `rg` to verify old public names (`BaseApp`, `FrameAppShell`, `FrameAppState`, `FrameAppHooks`) do not remain in code or current docs except migration/OpenSpec history.

## 5. Verification

- [x] 5.1 Run `cargo fmt` or the repository formatting command.
- [x] 5.2 Run the repository Rust compile/check command from `justfile`.
- [ ] 5.3 Run at least one lightweight demo smoke test when local Vulkan/runtime environment permits.
- [x] 5.4 Validate OpenSpec status for `simplify-render-app-shell`.
