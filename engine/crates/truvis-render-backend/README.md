# truvis-render-backend

渲染后端整合层，负责持有 `World` / `RenderWorld` 并通过生命周期方法暴露 typed Ctx。

## 主要职责

- 持有 `Gfx` root owner，并作为 typed Gfx Ctx 的生命周期来源
- 提供 `begin_frame`、`update_phase`、`prepare`、`render_phase`、`present`、`end_frame`
- 产出 `RenderBackendInitCtx` / `RenderBackendUpdateCtx` / `RenderBackendRenderCtx` / `RenderBackendResizeCtx` / `RenderBackendShutdownCtx`
- 与 swapchain / command 提交 / 同步机制对接

## 生命周期边界

- `RenderBackend::new` 创建 `Gfx` 并通过 typed Ctx 初始化 backend-owned GPU 资源。
- `RenderBackend::init_after_window` 创建 surface、swapchain 与 `RenderPresent`，并把 init 阶段所需的 typed Ctx 交给 app/plugin。
- `render_phase`、`handle_resize`、`shutdown_phase` 只暴露当前阶段需要的 device/resource/queue/surface/immediate/device-info Ctx。
- `wait_idle` 由 runtime 在 app/plugin shutdown 前调用，确保 plugin-owned pipeline、buffer、descriptor 等资源销毁前 GPU 不再引用上一帧 command buffer。
- `destroy` 先等待 GPU idle，再释放 present、FIF、assets、GPU scene、command allocator、sync、descriptor 等子资源，最后销毁 `Gfx` root owner。

## 与其他模块关系

- 上承 `truvis-frame-runtime::RenderAppShell`（帧骨架）与 `truvis-app`（插件编排）
- 下接 `truvis-gfx`、`truvis-render-interface`、`truvis-render-graph`
- 不依赖 `RenderApp`、`Plugin`、GUI plugin 或具体 demo app
