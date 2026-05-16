# truvis-world

`truvis-world` 定义 CPU 侧世界状态，是 scene 与 asset 数据进入渲染后端前的聚合点。

## 主要职责

- `World` 持有 `SceneManager`，负责 CPU 侧场景语义数据。
- `World` 持有 `AssetHub`，负责 asset 数据入口。
- 上层 update / prepare 阶段通过 `World` 访问 CPU 数据，再由 `RenderBackend::prepare` 同步到 GPU 可见资源。

## 边界约束

- `World` 不持有 Vulkan、`Gfx`、`RenderWorld` 或 swapchain 资源。
- `World` 不依赖 `truvis-render-backend`、`truvis-frame-api` 或 App/Plugin 契约。
- GPU frame state、bindless、global descriptor、manager-owned image/view 和 FIF resources 属于 `truvis-render-interface::render_world::RenderWorld`。

## 设计意图

`World` / `RenderWorld` 的拆分让 CPU 语义数据和 GPU 执行状态有清晰边界。App 和 Plugin 在 update 阶段修改 CPU 世界；backend 在 prepare 阶段把需要的 scene/asset 数据同步到 `RenderWorld` 管理的 GPU 资源；render 阶段主要读取 `RenderWorld` 录制命令。
