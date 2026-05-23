# truvis-scene

CPU 侧场景数据模块，管理 runtime instance / light 等场景语义数据。

## 核心职责

- runtime instance 与 light 存储、检索和生命周期维护
- instance 到 `AssetMeshHandle` / `AssetMaterialHandle` 的引用关系维护
- 将 ready model asset / prefab spawn 为 runtime instances
- 提供 CPU-only 程序化 mesh 数据，辅助构建测试场景或工具场景

## 与渲染关系

- 负责 CPU 语义数据，不直接承担底层 GPU 执行逻辑
- runtime `Instance` 直接引用 `AssetMeshHandle` 和 `AssetMaterialHandle`
- `InstanceHandle` / `LightHandle` 是 CPU runtime 身份，不等同于 GPU scene slot
- `SceneManager::spawn_model` 根据 `ModelData` 创建新的 live `InstanceHandle`
- render-side `InstanceBridge` 负责把 `InstanceHandle` 转换为稳定 `GpuInstanceSlot`
- 移除 instance 后，render-side `InstanceBridge` 会在后续 prepare/sync 阶段识别 stale handle 并回收 GPU-side 映射
- render-side scene bridge trait 负责把 material/mesh asset handle 解析为 GPU-ready 数据，
  这些契约定义在 `truvis-render-backend` 内部，不属于 scene crate
- texture / bindless 解析和 material buffer dirty 上传由 render-side material bridge 处理，`SceneManager` 不直接解析 shader 可见 binding
- mesh 的 vertex/index buffer 和 BLAS 由 render-side `AssetMeshUploader` 持有
- 本 crate 不依赖 `truvis-render-interface`，不持有 mesh/material GPU manager，也不暴露旧
  `MeshHandle` / `MaterialHandle` 兼容身份
- `procedural_mesh` 只生成 `MeshData` 和稳定 `AssetMeshKey`，调用方仍需通过 `AssetHub`
  注册后进入标准上传路径
