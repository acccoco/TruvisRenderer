# truvis-world

`truvis-world` 定义 CPU 侧世界状态，是 scene 与 asset 数据进入渲染运行时前的聚合点。

## 主要职责

- `World` 持有 `SceneStore`，负责 CPU 侧场景语义数据。
- `World` 持有 `AssetHub`，负责 asset 数据入口。
- `World` 持有 `SceneAssetIngestor`，负责把 App-facing scene import 请求映射到内部 asset
  loader 状态。
- `World` 提供 App-facing facade：App 通过它请求 model import、注册 texture/procedural mesh/material、
  注册 runtime instance、更新 sky state 和 analytic light，不直接组合 `AssetHub` 与 `SceneStore` 的内部调用顺序。
- `World` 的 scene edit API 使用 `WorldEditError` 显式报告 stale handle、缺失依赖、仍被引用和
  filesystem canonicalize 失败；失败 edit 不写 change log，也不污染依赖索引。
- file texture 通过 `World::register_texture` 进入 scene 前会先执行 filesystem canonicalize；model 主路径和
  model 内 texture 路径也在 `SceneAssetIngestor` 中 canonicalize，失败时 model import 进入 failed 状态。
- `World` 提供 render runtime-facing 窄接口：runtime 通过 `sync_for_render()` 消费已经翻译为
  CPU resource handle 的 `WorldRenderSync` typed payload 和 `SceneChanges`，通过只读 `scene_view()` 快照同步
  instance/light，不直接访问 `SceneStore` owner 或 `World` 内部字段。
- 上层 update / prepare 阶段通过 `World` 访问 CPU 数据，再由 `RenderRuntime::prepare` 同步到 GPU 可见资源。
- `SceneStore` 中的 handle 是 CPU runtime 身份；`AssetHub` 中的 handle 只作为 loader 内部身份，不表示 GPU slot 或 bindless index，也不扩散到 App / render-side manager。

## 边界约束

- `World` 不持有 Vulkan、`Gfx`、GPU resource/binding owner 或 swapchain 资源。
- `World` 不持有 GPU buffer、image、BLAS、material slot 或 frame state。
- `World` 不依赖 `truvis-render-runtime`、`truvis-app-frame` 或 App/Plugin 契约。
- `World` facade 对 model import 暴露 `ModelImportHandle`；内部 loader handle 只由
  `SceneAssetIngestor` 用于事件翻译，不把 loader 身份扩散到 App、`SceneStore` 或 render-side manager。
- `SceneStore`、`Instance`、raycast hit 和 `RenderWorld` manager 的长期引用使用 `TextureHandle` /
  `MeshHandle` / `MaterialHandle`，不使用 `Asset*Handle` 作为兼容层。
- `SceneStore` 内部维护 texture -> material、material -> instance 和 mesh -> instance 反向依赖索引；
  删除 texture/material/mesh 前先检查依赖，存在依赖时拒绝删除并返回 edit error。
- `SceneStore` 持有 `SceneSkyState`，记录 sky enabled、intensity、引用的 `TextureHandle` 和 revision；
  删除 texture 时也会检查 sky 是否仍引用该 texture。
- `SceneStore` 与 `AssetHub` 字段对外保持私有；只有 `World` 方法可以组合二者。
- `SceneStore` owner 不作为跨 crate 构造参数暴露；`World::new()` 负责创建内部 `SceneStore`、
  `AssetHub` 和 `SceneAssetIngestor`。
- GPU frame state、bindless、global descriptor 和 manager-owned image/view 属于 render-side runtime owner；具体窗口尺寸 render target 由 app 层 pipeline/plugin 持有。

## 设计意图

`World` / render-side GPU owner 的拆分让 CPU 语义数据和 GPU 执行状态有清晰边界。App 和 Plugin 在 update 阶段修改 CPU 世界；runtime 在 prepare 阶段把需要的 scene/asset 数据同步到 GPU resources 和 shader-visible bindings；render 阶段主要读取 `RenderPassRecordCtx` 录制命令。
