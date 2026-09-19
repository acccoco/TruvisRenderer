# truvis-world

`truvis-world` 定义 CPU 侧世界状态，是资源和 scene 数据进入渲染运行时前的聚合点。

## 所有权

- `AssetSystem` 独占 `AssetStore`、`AssetLoadService`、scene import 状态和 texture 去重表。
  它把 `RawSceneData` 转成 `SceneData`，不接收也不保存 `SceneStore`。
- `SceneStore` 独占 `MeshInstanceHandle`、`LightHandle`、sky 状态以及 instance/material/mesh
  反向引用。
- `GameWorld` 组合两个 owner，并在需要同时检查资源引用和场景引用时编排顺序。
- RenderRuntime 只通过 `SceneReadView` 借用最终 CPU 状态；GPU image、buffer、BLAS、descriptor
  和 frame state 属于 render-side owner。

## 对外流程

```text
GameWorld::import_texture(path, color_space)
  -> canonicalize
  -> AssetSource::File + color space 去重
  -> TextureAssetHandle

GameWorld::import_mesh(mesh_data)
  -> AssetStore
  -> MeshAssetHandle

GameWorld::import_scene(path)
  -> SceneImportHandle
  -> AssetLoadService
  -> AssetSystem::scene_data(handle)
  -> GameWorld::create_mesh_instance(SceneObjectData)
  -> MeshInstanceHandle
```

scene import 完成不会自动修改 `SceneStore`。外部可以过滤、复制或重复实例化
`SceneData.objects`。`SceneImportHandle` 的 `LoadStatus::Ready` 只表示 scene 结构和资源身份
已经写入 CPU owner；纹理解码、GPU 上传和最终画面仍是独立阶段。

## AssetSource 与去重

```rust
pub enum AssetSource {
    File { path: PathBuf },
    Embedded { scene_path: PathBuf, image_index: u32 },
    Generated { key: GeneratedSourceKey },
}
```

`TextureRecord` 保存 source、`TextureColorSpace`、CPU 状态和可选 `TextureBytes`。文件路径
进入 `AssetStore` 前 canonicalize；embedded source 使用所属 scene 的 canonical path 和
内部 image index。`AssetSource + TextureColorSpace` 是唯一 texture 去重 key，handle 只表示
资源本身，source metadata 用于诊断、重载和持久化。

## 删除与生命周期

`SceneStore` 先检查 live instance/sky 引用，`AssetStore` 检查 material/texture 依赖，
`GameWorld` 再执行 CPU 删除。CPU 删除不等于 GPU 立即销毁；render-side 继续按 FIF 和
completion 延迟回收。mesh/texture 内容不可变，内容变化创建新 handle。

架构约束和导入时序见 [`asset-system-boundaries.md`](../../../docs/brain-storm/asset-system-boundaries.md)。
