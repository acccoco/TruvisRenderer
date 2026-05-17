# truvis-asset

资产加载模块，提供纹理、mesh、material、scene 等内容资产的 CPU 侧身份、去重、加载状态与完成事件。

本模块位于 World 层和 RenderBackend 之间，只负责到 upload-ready CPU data：
不创建 GPU image / image view，不创建 vertex/index buffer、BLAS，也不注册
bindless descriptor 或 material slot。GPU 上传和 shader 可见绑定由
`truvis-render-backend` 的 `AssetTextureUploader`、`AssetMeshUploader`、
`MaterialBridge` 负责；scene asset / prefab 被 `SceneManager` 显式 spawn 后才会变成
runtime instance。

## 主要组件

- `AssetHub`：对外统一入口，负责路径/key 去重、handle 分配、状态表和完成事件汇聚
- `AssetLoadedEvent`：CPU 数据完成事件，交给渲染后端继续上传或交给 scene 层 spawn
- `TextureBytes`：从图片文件解码出的 owned CPU 纹理 bytes，只通过事件交给 uploader
- `MeshData`：从导入器复制出来的 owned CPU mesh 数据，只通过事件交给 mesh uploader
- `MaterialData`：导入后的 CPU material 参数和 texture handle 引用
- `SceneData`：导入后的 scene / prefab CPU 数据和内部 asset handle 引用
- `AssetMeshKey`：同一导入源内的 mesh 去重 key
- `AssetMaterialKey`：同一导入源内的 material 去重 key
- `AssetSceneKey`：scene 导入源路径去重 key

## 内部结构

- `asset_loader`：crate 内部后台调度层，只持有 Rayon 线程池、结果 channel 和任务等待逻辑。
- `texture_loader`：crate 内部纹理任务实现，只负责 image 文件读取、CPU 解码和 RGBA8 bytes 输出。
- `truvixx_scene_loader`：crate 内部 scene 导入任务实现，只负责 C++ importer 生命周期和 owned CPU scene 数据复制。
- 外部调用方不直接使用 loader 模块；加载请求、状态查询和完成事件都通过 `AssetHub` 进入或离开 asset 层。

## 设计目标

- 避免阻塞渲染主流程
- 通过状态机管理 Loading -> Ready / Failed 过程，其中 Ready 只表示 CPU ready
- `AssetTextureHandle` 表示内容纹理身份，不表示 GPU image/view 或 bindless index
- `AssetMeshHandle` 表示内容 mesh 身份，不表示 vertex/index buffer 或 BLAS ready
- `AssetMaterialHandle` 表示内容材质身份，不表示 GPU material slot
- `AssetSceneHandle` 表示 scene asset / prefab，不表示 live runtime instance
- Assimp 导入任务只在后台复制 owned CPU 数据，完成后释放 C++ scene handle，不把 C++ handle/raw pointer 传出任务
- Assimp 导入失败会读取 C++ importer 的详细错误并转为 `SceneFailed` 事件
- scene material 引用的相对纹理路径按 scene 文件所在目录解析，绝对路径保持不变；路径只做词法归一化，不访问文件系统
- 保持 asset 层不依赖 GPU 资源缓存或 bindless 绑定策略
