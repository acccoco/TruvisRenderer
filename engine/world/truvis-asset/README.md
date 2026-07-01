# truvis-asset

资产加载模块，提供 texture / model 的一次性 CPU loader task 和完成事件。

本模块位于 World 层和 RenderRuntime 之间，只负责到 upload-ready CPU data：
不创建 GPU image / image view，不创建 vertex/index buffer、BLAS，也不注册
bindless descriptor 或 material slot。GPU 上传和 shader 可见绑定由
`truvis-render-runtime` 的 `RenderTextureManager`、`RenderMeshManager`、
`RenderMaterialManager` 负责；model import 由 `World` 内部的
`SceneAssetIngestor` 在 asset sync 阶段实例化成 runtime instance。App 层不直接持有
`ModelLoadHandle`，而是通过 `World` 的 `ModelImportHandle` facade 查询 model import。

## 主要组件

- `AssetHub`：对外统一入口，负责 loader handle 分配和完成事件汇聚
- `AssetLoadEvent`：CPU 数据完成事件，交给 `SceneAssetIngestor` 翻译成 CPU resource handle 和 render upload event
- `TextureLoadDesc` / `ModelLoadDesc`：一次性 loader task 输入描述，不承担长期去重 identity
- `TextureBytes`：从图片文件解码出的 owned CPU 纹理 bytes，只通过事件交给 texture manager
- `MeshData`：从导入器复制出来的 owned CPU mesh 数据，只通过事件交给 mesh manager
- `RawSceneData`：model 导入后的 owned CPU scene payload，通过 `ModelLoaded` 事件交给 `SceneAssetIngestor`

## 内部结构

- `asset_loader`：crate 内部后台调度层，只持有 Rayon 线程池、结果 channel 和任务等待逻辑。
- `texture_loader`：crate 内部纹理任务实现，只负责 image 文件读取、CPU 解码和 RGBA8 bytes 输出。
- `truvixx_scene_loader`：crate 内部 Assimp scene 导入任务实现，通过 `truvis-assimp-binding` 调用 Assimp C API，只负责 C++ importer 生命周期和 owned CPU scene 数据复制。
- `gltf_scene_loader`：crate 内部 glTF / GLB scene 导入任务实现，通过 Rust `gltf` crate 读取 material / mesh / instance，并复制成与 Assimp 路径相同的 owned CPU scene 数据。
- 外部调用方不直接使用 loader 模块；加载请求、状态查询和完成事件都通过 `AssetHub` 进入或离开 asset 层。

## 设计目标

- 避免阻塞渲染主流程
- 通过状态机管理 Loading -> Ready / Failed 过程，其中 Ready 只表示 CPU ready
- `TextureLoadHandle` 表示 texture load task 身份，不表示长期 scene texture 或 GPU binding
- `ModelLoadHandle` 表示 model load task 身份，不表示长期 prefab 或 live runtime instance
- Assimp 导入任务只在后台复制 owned CPU 数据，完成后释放 C++ scene handle，不把 C++ handle/raw pointer 传出任务
- glTF 导入任务只在后台复制 owned CPU 数据；`.gltf` / `.glb` 由 asset loader 按扩展名分派，其它格式继续走 Assimp 路径
- Assimp / glTF 导入失败会通过 `ModelFailed` 事件回传给 `SceneAssetIngestor`
- model material 引用的相对纹理路径按 model 文件所在目录解析，绝对路径保持不变；asset 层不做 scene texture identity 去重或 canonicalize，后续是否规范化由 `World` / `SceneAssetIngestor` 的 scene 规则决定。glTF v1 只把外部 image URI 注册为 texture path，GLB/data URI 嵌入贴图暂不改变 texture path 身份模型。
- 保持 asset 层不依赖 GPU 资源缓存或 bindless 绑定策略
