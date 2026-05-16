# truvis-scene

CPU 侧场景数据模块，管理 mesh/material/instance/light 等实体数据。

## 核心职责

- 场景组件存储与检索
- 场景实例组织与引用关系维护
- 生成供渲染上传使用的数据视图

## 与渲染关系

- 负责 CPU 语义数据，不直接承担底层 GPU 执行逻辑
- 与 `truvis-render-interface` 的数据契约协作完成上传
- runtime `Instance` 直接引用 `AssetMeshHandle` 和 `AssetMaterialHandle`
- render-side `InstanceBridge` 负责把 `InstanceHandle` 转换为稳定 `GpuInstanceSlot`
- `MaterialSlotResolver` 把 `AssetMaterialHandle` 转换为稳定 GPU material slot
- `MeshRenderResolver` 用于判断 `AssetMeshHandle` 是否 GPU-ready
- texture / bindless 解析和 material buffer dirty 上传由 render-side material bridge 处理，`SceneManager` 不直接解析 shader 可见 binding
- mesh 的 vertex/index buffer 和 BLAS 由 render-side `AssetMeshUploader` 持有
