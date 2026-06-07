# Asset / Scene Pipeline 当前状态

> 状态：活跃摘要，更新于 2026-05-23。当前事实以
> [`docs/ARCHITECTURE.md`](../ARCHITECTURE.md) 和代码为准。

本文记录资产、场景和 GPU scene 数据流的当前主线，替代早期 asset/bindless 与 scene 迁移草案。

## 当前决策

- `AssetHub` 只表达内容资产身份、去重、CPU 加载状态和加载完成事件。
- `AssetHub` 的 `Ready` 只表示 CPU 数据可读，不表示 GPU image、BLAS、material slot 或 bindless descriptor 已就绪。
- GPU 上传、image/view 创建、bindless 注册、BLAS 构建、fallback 纹理和 shader 可见绑定都归 render-side owner。
- `SceneManager` 只保存 runtime instance / light 等 CPU 语义；GPU instance slot、ready gate 和 active render list 由 `InstanceBridge` 维护。
- `GpuScene` 与 `RenderData` 是 `truvis-render-runtime` 私有 scene 翻译层；pass 只能通过 `RenderSceneView` 读取。
- `AssetModelHandle` 表示可重复 spawn 的 prefab / model asset，不拥有 live `InstanceHandle` 生命周期。

## 当前数据流

```text
App update
  -> World.asset_hub.load_model/load_texture/register_*
  -> AssetHub 后台读取和 CPU 数据归一化
  -> AssetLoadedEvent
  -> RenderRuntime::dispatch_loaded_asset_events
      -> AssetTextureManager: texture GPU upload + bindless SRV + fallback resolve
      -> AssetMeshManager: vertex/index upload + BLAS build + mesh ready revision
      -> MaterialBridge: stable material slot + material buffer dirty
  -> App 在 model ready 后调用 SceneManager::spawn_model
  -> InstanceBridge 根据 mesh/material resolver 做 ready gate 和 stable instance slot
  -> GpuScene 上传 instance / geometry / light / TLAS / raster draw cache
  -> pass 通过 RenderSceneView 录制 draw / trace
```

## 已落地的边界收敛

- Asset 层不再依赖 `truvis-gfx`、render foundation manager 或 `BindlessManager`。
- texture path 在 AssetHub ingest scene 阶段按 scene 文件目录归一化。
- Assimp scene 读取已进入 AssetHub 后台加载路径，失败状态可传播到 model status。
- Material GPU slot 由 render-side bridge 管理，材质贴图通过 `TextureResolver` 解析 fallback 或真实 SRV。
- Mesh CPU 数据通过加载事件交给 `AssetMeshManager`，mesh GPU ready 后再允许相关 instance 激活。
- Instance slot 在 runtime 生命周期内稳定，despawn 后通过 FIF frame token 延迟回收。
- TLAS 当前使用 dirty/revision 后整棵 rebuild，不做第一阶段 refit。
- render pass 不再直接依赖 CPU world，也不在 draw 阶段构建 scene render data。

## 剩余方向

- 将 `RenderRuntime::prepare()` 拆成更显式的 extract / prepare 子阶段，便于定位 scene snapshot、bridge resolve 和 GPU upload 的职责。
- 评估 texture upload batching，减少大量 texture 同帧完成时的 command buffer / submit 次数。
- 评估 strict readiness 策略；当前 material texture 默认使用 fallback，保证 shader 始终有安全绑定。
- 资产热重载、跨场景引用计数卸载和 mesh / material 替换后的细粒度 invalidation 仍是后续能力。
- 如果未来更多 pass 需要 scene 快照，可再评估 `RenderData` owned 化或更明确的 render scene cache。
