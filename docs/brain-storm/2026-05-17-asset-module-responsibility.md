# Asset 模块职责收敛记录（2026-05-17）

## 决策

`truvis-asset` 只负责内容资产身份和文件到 CPU bytes 的加载流程：

```text
AssetHub
  path -> AssetTextureHandle
  file/decode -> LoadedTextureBytes
```

GPU 上传、image/view 创建、bindless 注册和 fallback 策略迁移到
`truvis-render-runtime::asset_texture_uploader::AssetTextureUploader`：

```text
AssetTextureUploader
  LoadedTextureBytes -> GfxImage/ImageView
  ImageView -> BindlessSrvHandle
  AssetTextureHandle -> TextureBinding
```

## SlotMap 归属

主 `SlotMap<AssetTextureHandle, TextureAssetRecord>` 由 `AssetHub` 持有，因为
`AssetTextureHandle` 表达的是内容资产身份。渲染侧只通过
`SecondaryMap<AssetTextureHandle, UploadedAssetTexture>` 记录某个 asset handle
当前对应的 GPU 绑定。

## 边界约束

- `AssetHub` 不依赖 `truvis-gfx`、`truvis-render-interface` 或 `BindlessManager`。
- `AssetLoader` 直接向 rayon 线程池提交文件读取/解码任务，不再使用额外 dispatch thread。
- `SceneManager::prepare_render_data()` 只依赖 `TextureResolver`，不再通过路径访问 `AssetHub`。
- fallback texture 是渲染侧策略，由 `AssetTextureUploader` 持有和解析。
