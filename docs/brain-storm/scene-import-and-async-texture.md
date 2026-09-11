# Scene 导入与异步 Texture 设计

## 目标

为 FBX、glTF、GLB 等 scene 文件建立一条统一且简单的导入流程，覆盖：

- scene 文件内的 mesh、node、material 和 instance 结构；
- material 通过相对路径或绝对路径引用的外部 texture；
- glTF data URI、GLB bufferView、Assimp embedded texture 等内嵌 texture；
- scene 结构先进入 `GameWorld`，texture 在后台异步读取和解码；
- texture 尚未完成时使用现有 fallback，完成后由正常 `prepare` 流程发布到 GPU。

设计只扩展现有 `AssetHub -> SceneAssetIngestor -> AssetStore/SceneStore` 边界，不引入全局 asset database、跨线程 `GameWorld`、引用计数框架或通用异步依赖图。

## 当前基线

当前实现事实见 [`../summaries/scene-data-lifecycle.md`](../summaries/scene-data-lifecycle.md) 和
[`../../engine/e30-world/truvis-asset/README.md`](../../engine/e30-world/truvis-asset/README.md)。核心边界如下：

```text
AssetHub / loader
    -> AssetLoadEvent（owned CPU payload）
SceneAssetIngestor
    -> AssetStore（TextureHandle / MeshHandle / MaterialHandle）
    -> SceneStore（instance / light / sky 关系）
RenderRuntime::prepare
    -> RenderAssetSystem / RenderWorld
```

glTF loader 复制 mesh、material、instance 和 embedded image 的 encoded bytes；`RawMaterialData` 通过 `RawTextureSource` 区分外部路径与 embedded source。外部 texture 由 `SceneAssetIngestor` canonicalize、按路径复用并异步解码，embedded texture 按 scene path + image identity 复用。当前 model import 的 `Ready` 主要表示 CPU scene ingest 完成，不应被解释为 texture 或 GPU 已 ready。

当前实现已落地阶段 1--2、阶段 3 的 glTF/GLB/data URI 路径和阶段 4--6 的统一 ingest 与事实文档同步：embedded image 以 owned encoded bytes 和 MIME 进入 `AssetHub::request_texture_bytes`，完成后沿用既有 `TextureLoaded` / `TextureFailed` 事件。Assimp binding 当前没有可安全复制的 embedded image API，因此仍只保留外部 texture path；这属于文档允许的后续能力，不改变首期资源边界。

## 设计理念与不变量

1. loader 只解析和复制，不分配长期 scene handle，不修改 `GameWorld`，不创建 Vulkan 对象。
2. `AssetStore` 是长期 CPU texture 内容和 identity 的 owner；`SceneStore` 只通过 handle 建立关系。
3. texture identity 在注册时确定，异步完成只改变 `Loading -> Ready/Failed`，不替换 handle。
4. 外部 texture 和 embedded texture 都转换成同一个 `TextureHandle`，material 不关心来源。
5. scene structure ready、texture CPU ready、GPU upload submitted、GPU binding published 是独立阶段。
6. mesh / texture 内容继续保持创建后不可变；不同内容创建新 handle。
7. scene 导入首期不做事务回滚。结构导入失败报告 import failure；中途已注册且无引用的资源沿用普通孤儿清理路径。
8. 单张 texture 失败不撤销已经可用的 scene structure；渲染使用 fallback，并保留 texture 级错误。

## 最小数据模型

### 1. 统一 texture source

在 `truvis-asset` 的 raw scene 边界新增一个小枚举，不为每种格式建立独立 material 类型：

```rust
pub enum RawTextureSource {
    ExternalPath(PathBuf),
    Embedded {
        identity: EmbeddedTextureId,
        bytes: Arc<[u8]>,
        mime_type: Option<String>,
    },
}
```

`RawMaterialData` 将 `diffuse_texture_path`、`normal_texture_path` 替换为对应的 `Option<RawTextureSource>`。`EmbeddedTextureId` 只需要在一个 scene 导入中稳定，建议由 canonical scene path 加 image index 或 bufferView 范围组成；首期不做跨 scene 内容哈希去重。

### 2. 两种 texture task 输入

`AssetHub` 增加与现有 file request 对称的内存 request：

```text
request_texture(path)
request_texture_bytes(identity, bytes, mime_type)
```

两者都返回 `TextureLoadHandle`，并通过现有 `TextureLoaded { data: TextureBytes }` / `TextureFailed` 事件返回结果。`AssetHub` 只保存 task 状态和事件，不保存长期 `TextureHandle` 或像素 registry。

### 3. Scene import 状态

保持现有状态机简单，不建立独立的 texture dependency graph。建议将 model import 记录扩展为：

```text
LoadingStructure
StructureReady
Failed
```

`StructureReady` 的定义是 mesh、material、instance 已写入 CPU owner；texture 可能仍处于 Loading。若 UI 需要整体进度，由 `SceneAssetIngestor` 统计 pending/failed texture 数量，而不是让 AssetHub 维护 scene 级状态。

## 推荐调用链

```text
GameWorld::request_model_import(path)
  -> SceneAssetIngestor::request_model_import
  -> AssetHub::request_model
  -> worker 解析 scene，复制 RawSceneData
  -> ModelLoaded(RawSceneData)

GameWorld::poll_asset_loads()
  -> AssetHub::update()
  -> SceneAssetIngestor::ingest_model_loaded()
     1. 校验 raw mesh / instance / material index
     2. 注册 MeshHandle
     3. 将每个 RawTextureSource 转成 TextureHandle(Loading)
     4. 注册 MaterialHandle
     5. 注册 SceneStore instance
     6. import 状态改为 StructureReady

后续若干帧
  -> AssetHub::update()
  -> TextureLoaded / TextureFailed
  -> SceneAssetIngestor
  -> AssetStore::mark_texture_loaded/failed
  -> RenderRuntime::prepare
  -> RenderAssetSystem::sync
  -> fallback 或真实 GPU texture binding
```

## 分阶段实施计划

### 阶段 0：冻结边界和记录当前事实

- 确认 `RawSceneData` 是 loader 到 world 的唯一 owned payload。
- 确认 `TextureHandle` 是 CPU identity，不把 `TextureLoadHandle` 或 GPU slot 暴露给 Renderer。
- 保持现有外部路径行为、路径 canonicalize、按 canonical path 复用和非事务导入语义。
- 本阶段只补设计文档，不改运行时代码。

完成条件：调用链、owner、线程和失败语义与 `scene-data-lifecycle.md` 一致。

### 阶段 1：扩展 raw texture source

修改范围：`engine/e30-world/truvis-asset/src/handle.rs` 及其导出。

- 新增 `RawTextureSource` 和最小的 `EmbeddedTextureId`。
- 将 `RawMaterialData` 的两个 path 字段改为 source 字段。
- 保持 `RawSceneData`、mesh 和 instance index 结构不变。
- 为 source 增加必要的 `Debug/Clone/PartialEq`，不新增泛型 trait 或格式专用抽象。

验证：`cargo check -p truvis-asset -p truvis-world`，并修复所有编译期匹配分支。

### 阶段 2：增加 embedded texture loader request

修改范围：`asset_loader.rs`、`asset_hub.rs`、texture loader。

- 增加内存 bytes request；worker 从 owned bytes 解码为现有 `TextureBytes`。
- file 和 memory 两条路径共享解码函数，避免复制两套像素格式逻辑。
- worker 仍通过 `catch_unwind` 转换为失败事件；不把 loader 内部对象或借用 slice 传出。
- 事件协议继续使用 `TextureLoadHandle`，不新增 scene 专用 completion queue。

验证：使用一张普通 RGBA 图片和一张 embedded payload 验证 `TextureBytes` extent、格式和像素数量校验。

### 阶段 3：保留 glTF / GLB embedded image

修改范围：`gltf_scene_loader.rs`。

- 不再丢弃 `gltf::import` 返回的 image 数据。
- 外部 URI 输出 `ExternalPath`；data URI 和 GLB bufferView 输出 `Embedded`。
- 同一 glTF image 被多个 material 引用时，复用同一个 source identity。
- 仍保持当前 primitive 扁平化、node transform 和 material 能力范围，不顺手扩展未支持的 glTF material 特性。

修改范围：`truvixx_scene_loader.rs`（若 Assimp binding 已提供 embedded image 数据）。

- 若能取得 embedded bytes，映射为 `Embedded`。
- 若当前 binding 无法安全复制 embedded 数据，明确记录为后续能力，不在本阶段修改 FFI 结构。

验证：至少覆盖 `.gltf + 外部 image`、`.glb + bufferView image`、data URI 三类输入；确认 loader 返回 owned bytes，任务结束后不依赖 glTF document/image 对象。

### 阶段 4：收敛 `SceneAssetIngestor` 注册逻辑

修改范围：`scene_asset_ingestor.rs`、`asset_system.rs`。

- 保留现有 `canonical path -> TextureHandle` 表。
- 增加 `embedded identity -> TextureHandle` 表，或将两者收敛到一个内部 `TextureKey`；不引入通用 key trait。
- 外部 source：解析相对 scene 路径、canonicalize、请求 file texture。
- Embedded source：直接请求 memory texture。
- 两种 source 最终都写入 `texture_loads: TextureLoadHandle -> TextureHandle`。
- material 注册仍只验证 handle 存在，不等待 texture ready。
- model import 在 instance 注册成功后进入 `StructureReady`；texture completion 独立更新 `AssetStore`。
- 删除 texture 时同时清理对应 external/embedded identity 表项。

验证：重复外部路径和重复 embedded identity 只产生一个 live `TextureHandle`；删除后 identity 不再命中旧 handle；单纹理失败不会删除 material/instance。

### 阶段 5：补充状态查询和诊断

只增加实际需要的查询，不建立进度系统：

- `model_import_status()` 继续返回结构导入状态。
- 可选增加 `model_import_texture_summary()`，返回 pending/ready/failed 数量。
- 日志区分 model structure failure、external path failure、embedded decode failure。

验证：状态变化顺序为 `LoadingStructure -> StructureReady`，纹理失败不会把结构状态错误改回 Failed；日志能关联 model import、texture identity 和 source。

### 阶段 6：同步事实文档

实现完成后再更新：

- `engine/e30-world/truvis-asset/README.md`：说明 external/embedded source 和两个 texture request。
- `docs/summaries/scene-data-lifecycle.md`：记录 structure ready 与 texture/GPU ready 的分层语义。
- 本文保留后续设计边界，不复制已经验证的代码细节。

## 失败、取消和删除语义

首期不增加取消协议。用户再次导入同一 scene 时创建新的 model import record；旧任务完成后仍按原 handle ingest，若上层需要替换场景，由上层显式删除旧 instance 和无引用资源。

导入过程中发生结构错误时，`SceneAssetIngestor` 将 import 标记为 Failed。由于首期不做事务回滚，已注册资源可能暂时成为孤儿；资源删除继续经过现有反向引用检查。texture decode 失败只标记对应 texture Failed，material 保留稳定 handle，渲染侧使用 fallback。

## 明确的非目标

- 不引入统一 AssetDatabase、资源包系统、引用计数或跨场景缓存。
- 不引入通用 `AssetSource` trait、泛型 importer pipeline 或可插拔 dependency graph。
- 不在本方案中实现 texture 内容热替换、GPU 资源压缩、mipmap streaming 或按优先级调度。
- 不把 texture decode、GPU upload 和 RenderThread 并行化为新的线程协议。
- 不扩展 glTF 全部材质语义；只覆盖当前 `MaterialData` 能表达的字段。
- 不改变 `RenderRuntime::prepare`、`RenderAssetSystem` 和 `RenderWorld` 的 GPU owner 与 ready gate。

## 完成标准

- 外部 path、data URI、GLB bufferView 至少各有一个可重复的 loader/ingest 验证样例。
- scene structure 在 texture 尚未完成时已出现在 `GameWorld`，对应 instance 可以进入现有 Pending/fallback 渲染路径。
- texture 完成后无需修改 instance 或 material handle，后续 prepare 能发现 ready revision 并发布真实 binding。
- 单纹理失败不会破坏 scene structure；结构失败、texture 失败和 GPU 未完成在日志和状态上可区分。
- 不新增跨层 owner；`AssetHub`、`AssetSystem`、`SceneStore`、`RenderAssetSystem` 的职责仍与架构入口一致。
- 代码、模块 README、summary 和本 brain-storm 文档之间没有互相冲突的状态定义。
