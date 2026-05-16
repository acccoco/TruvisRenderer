# truvis-scene

CPU 侧场景数据模块，管理 runtime instance / light 等场景语义数据。

## 核心职责

- runtime instance 与 light 存储、检索和生命周期维护
- instance 到 `AssetMeshHandle` / `AssetMaterialHandle` 的引用关系维护
- 将 ready scene asset / prefab spawn 为 runtime instances
- 提供 CPU-only 程序化 mesh 数据，辅助构建测试场景或工具场景

## 与渲染关系

- 负责 CPU 语义数据，不直接承担底层 GPU 执行逻辑
- 与 `truvis-render-interface` 的数据契约协作完成上传
- runtime `Instance` 直接引用 `AssetMeshHandle` 和 `AssetMaterialHandle`
- `SceneManager::spawn_scene_asset` 根据 `LoadedSceneData` 创建新的 live `InstanceHandle`
- render-side `InstanceBridge` 负责把 `InstanceHandle` 转换为稳定 `GpuInstanceSlot`
- `MaterialSlotResolver` 把 `AssetMaterialHandle` 转换为稳定 GPU material slot
- `MeshRenderResolver` 用于判断 `AssetMeshHandle` 是否 GPU-ready
- texture / bindless 解析和 material buffer dirty 上传由 render-side material bridge 处理，`SceneManager` 不直接解析 shader 可见 binding
- mesh 的 vertex/index buffer 和 BLAS 由 render-side `AssetMeshUploader` 持有
- 本 crate 不持有 mesh/material GPU manager，也不暴露旧 `MeshHandle` / `MaterialHandle` 兼容身份
- `procedural_mesh` 只生成 `LoadedMeshData` 和稳定 `MeshAssetKey`，调用方仍需通过 `AssetHub`
  注册后进入标准上传路径
