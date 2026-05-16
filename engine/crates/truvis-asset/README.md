# truvis-asset

资产加载模块，提供纹理、mesh、material、scene 等内容资产的 CPU 侧身份、去重、加载状态与完成事件。

本模块只负责到 upload-ready CPU bytes，不创建 GPU image / image view，
不创建 vertex/index buffer、BLAS，也不注册 bindless descriptor。GPU 上传和 shader
可见绑定由 `truvis-render-backend` 的 `AssetTextureUploader` / `AssetMeshUploader`
负责。

## 主要组件

- `AssetHub`：对外统一入口
- `AssetLoader`：后台 IO、CPU 解码与 Assimp scene 导入
- `LoadedAssetEvent`：CPU 数据完成事件，交给渲染后端继续上传
- `LoadedMeshData`：从导入器复制出来的 owned CPU mesh 数据
- `LoadedMaterialData`：导入后的 CPU material 参数和 texture handle 引用
- `LoadedSceneData`：导入后的 scene / prefab CPU 数据和内部 asset handle 引用
- `MeshAssetKey`：同一导入源内的 mesh 去重 key
- `MaterialAssetKey`：同一导入源内的 material 去重 key
- `SceneAssetKey`：scene 导入源路径去重 key

## 设计目标

- 避免阻塞渲染主流程
- 通过状态机管理 Loading -> Ready / Failed 过程，其中 Ready 只表示 CPU ready
- `AssetMaterialHandle` 表示内容材质身份，不表示 GPU material slot
- `AssetSceneHandle` 表示 scene asset / prefab，不表示 live runtime instance
- Assimp 导入任务只在后台复制 owned CPU 数据，完成后释放 C++ scene handle
- 保持 asset 层不依赖 GPU 资源缓存或 bindless 绑定策略
