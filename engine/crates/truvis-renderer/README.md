# truvis-renderer

渲染后端整合层，负责持有 `World` / `RenderWorld` 并通过生命周期方法暴露 typed Ctx。

## 主要职责

- 提供 `begin_frame`、`update_phase`、`prepare`、`render_phase`、`present`、`end_frame`
- 产出 `RendererInitCtx` / `RendererUpdateCtx` / `RendererRenderCtx` / `RendererResizeCtx`
- 与 swapchain / command 提交 / 同步机制对接

## 与其他模块关系

- 上承 `truvis-frame-runtime::BaseApp`（帧骨架）与 `truvis-app`（插件编排）
- 下接 `truvis-gfx`、`truvis-render-interface`、`truvis-render-graph`
- 不依赖 `FrameApp`、`Plugin`、GUI plugin 或具体 demo app
