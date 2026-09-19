# truvis-asset

资产加载模块只负责一次性的 CPU 文件读取、纹理解码和 scene 导入。它位于 `AssetSystem`
和 RenderRuntime 之间，不创建 Vulkan image、buffer、BLAS、descriptor 或 material slot。

## 组件与边界

- `AssetLoadService` 分配 `TextureLoadHandle` / `SceneLoadHandle`，保存一次性请求描述，
  收集 worker 结果并发布 `AssetLoadEvent`。
- `AssetLoadWorker` 隐藏 Rayon 线程池和结果 channel。后台任务只读取文件、解码纹理、
  导入 scene 并复制 owned Rust 数据，不修改 `AssetStore`、`GameWorld` 或 `SceneStore`。
- `TextureLoadDesc` / `SceneLoadDesc` 是 task 输入，不承担长期资源身份；`TextureBytes`、
  `MeshData` 和 `RawSceneData` 是 CPU completion payload。
- `AssetSystem` 消费事件并分配 `TextureAssetHandle`、`MeshAssetHandle`、
  `MaterialAssetHandle`；scene 完成后发布 `SceneData`，外部再创建 `MeshInstanceHandle`。

`TextureLoadHandle` 和 `SceneLoadHandle` 只在加载阶段存在。它们不表示长期资源、GPU slot
或 live runtime instance。CPU `Ready` 也不表示 GPU upload 或 descriptor publish 已完成。

## Scene 与纹理输入

Assimp / glTF loader 只复制 owned scene 数据。`RawTextureSource` 保留外部路径或 embedded
bytes；路径解析、canonicalize、`AssetSource` 构造以及
`AssetSource + TextureColorSpace` 去重由 `truvis-world` 的 `AssetSystem` 负责。
Embedded source 使用 canonical scene path 与 image index 表示。

图片解码不翻转行；glTF UV 保持原值，Assimp 继续由 C++ 导入阶段处理坐标约定。
`TextureBytes` 校验 extent 和四通道元素数量，并根据 payload 推导 Vulkan format；HDR/EXR
使用线性 RGBA16F，普通图片按显式 `Srgb`/`Linear` 解释为 RGBA8。

## 相关入口

- `asset_load_service.rs`：服务和 completion event。
- `asset_load_worker.rs`：后台任务调度。
- `handle.rs`：load desc、load handle、CPU payload 和 raw scene 类型。
- `gltf_scene_loader.rs` / `truvixx_scene_loader.rs`：格式导入任务。
- `texture_loader.rs`：图片解码任务。
- 根目录 [`scripts/scene/export_gltf.py`](../../../scripts/scene/export_gltf.py)：离线导出 glTF 及外部贴图；
  相机和 glTF 可表达的灯光随场景导出，不生成独立的 manifest 或 metadata JSON。

长期资源 source、去重和 scene import 状态见 [`truvis-world`](../truvis-world/README.md) 与
[`scene-data-lifecycle.md`](../../../docs/summaries/scene-data-lifecycle.md)。
