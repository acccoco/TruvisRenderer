# Asset System 边界、身份与 Scene 导入

## 状态

本文是已采纳并正在维护的设计基线。当前实现事实仍以
[`docs/summaries/scene-data-lifecycle.md`](../summaries/scene-data-lifecycle.md)、模块 README
和源码为准；若实现暂时偏离本文，应明确记录偏差。

本文解决三个问题：

- 文件加载任务、CPU 资源、场景对象分别使用什么身份；
- Scene 文件如何从后台 loader 变成可查询的 `SceneData`；
- `AssetSystem` 与 `GameWorld` / `SceneStore` 如何保持单向边界。

## 目标结构

```text
AssetLoadService
└── AssetLoadWorker
    ├── TextureLoadHandle
    └── SceneLoadHandle
             │ AssetLoadEvent（owned CPU payload）
             ▼
AssetSystem
├── AssetStore
│   ├── AssetSource in AssetRecord
│   ├── TextureAssetHandle / MeshAssetHandle / MaterialAssetHandle
│   ├── AssetRecord 与 CPU payload
│   └── texture 去重表
└── SceneImportHandle -> SceneData
             │
             ▼
GameWorld
├── AssetSystem
└── SceneStore
    ├── MeshInstanceHandle
    └── LightHandle
             │
             ▼
RenderAssetSystem / RenderWorld / GPU
```

`AssetLoadService` 只处理一次性的文件加载任务。`AssetSystem` 是 CPU 资源和 scene import
状态的 owner。`GameWorld` 组合 `AssetSystem` 与 `SceneStore`，但 `AssetSystem` 不调用、也不
保存 `SceneStore`。

GPU image、buffer、BLAS、bindless slot、material slot 和 instance slot 仍属于 render-side
owner。CPU handle 不能被解释为 GPU 身份。

## 当前实现状态

当前代码已经完成这条边界迁移：`AssetHub`/`AssetLoader` 已分别改为
`AssetLoadService`/`AssetLoadWorker`，`SceneAssetIngestor` 已移除并并入 `AssetSystem`。
`AssetSystem` 消费 load event 后只发布 `SceneData`，不会向 `SceneStore` 创建 instance；
Renderer 根据 `SceneImportHandle` 查询 `SceneData`，再显式创建 `MeshInstanceHandle`。

资源 source、颜色空间、CPU payload 和 texture 去重表由 `AssetStore` 持有。下面的约束是
实现必须继续保持的设计不变量，summary 文档记录运行时当前事实。

## 身份划分

| 身份 | owner | 语义 |
| --- | --- | --- |
| `TextureLoadHandle` | `AssetLoadService` | 一次 texture 读取/解码任务 |
| `SceneLoadHandle` | `AssetLoadService` | 一次 FBX/glTF/GLB 文件导入任务 |
| `SceneImportHandle` | `AssetSystem` | 一次完整 scene import 流程，可查询状态和 `SceneData` |
| `AssetSource` | `AssetStore` / `AssetRecord` | 文件、embedded image 或 generated resource 的来源描述 |
| `TextureAssetHandle` | `AssetStore` | CPU texture 资源身份 |
| `MeshAssetHandle` | `AssetStore` | CPU mesh 资源身份 |
| `MaterialAssetHandle` | `AssetStore` | CPU material 资源身份 |
| `MeshInstanceHandle` | `SceneStore` | GameWorld 内的 mesh 场景对象身份 |
| `LightHandle` | `SceneStore` | GameWorld 内的光源身份 |

`SceneLoadHandle` 不得扩散为资源身份；`TextureAssetHandle` 不表示 Vulkan image 或
bindless descriptor；`MeshInstanceHandle` 不表示 mesh 资源。异步完成只更新已有 asset
record，不替换 handle。

## AssetLoadService

`AssetHub` 更名为 `AssetLoadService`，`AssetLoader` 更名为 `AssetLoadWorker`。

`AssetLoadService` 负责：

1. 分配 `TextureLoadHandle` / `SceneLoadHandle`；
2. 保存任务描述并提交给 worker；
3. 收集 worker 返回的 owned Rust 数据；
4. 发布 `AssetLoadEvent`；
5. 在销毁时等待 worker 任务结束。

worker 只读取文件、解码 texture、导入 scene 并复制出 `RawSceneData`。worker 不修改
`AssetStore`、`GameWorld` 或 `SceneStore`，也不创建 Vulkan 对象。事件在 update/prepare
线程被 `AssetSystem` 消费，所有 CPU 资源状态变更收敛到该线程。

`RawSceneData` 是 loader 到上层的临时 owned payload；它可以继续保留导入源中的 mesh、
material 和 instance index。它不是长期 scene identity，也不是 `SceneData`。

## AssetSource、AssetRecord 与去重

建议把 source metadata 放入资源记录，而不是放入资源 handle。当前目标不引入独立的
`SourceAssetId` 或 source registry：

```rust
pub enum AssetSource {
    File { path: PathBuf },
    Embedded { scene_path: PathBuf, image_index: u32 },
    Generated { key: GeneratedSourceKey },
}

pub struct TextureRecord {
    pub source: AssetSource,
    pub color_space: TextureColorSpace,
    pub state: TextureState,
    pub data: Option<TextureBytes>,
    pub error: Option<String>,
}
```

`GeneratedSourceKey` 必须定义稳定的相等语义：相同生成参数可以复用时，key 应包含
generator 类型和参数；每次生成都必须是新资源时，key 应包含唯一实例标识。

`AssetStore` 维护：

- asset handle -> `AssetRecord`；
- `TextureKey { source: AssetSource, color_space: TextureColorSpace } -> TextureAssetHandle`；
- material -> texture 的反向依赖。

文件 source 在进入 `AssetStore` 前 canonicalize。embedded source 使用所属 glTF 文件的
canonical path 和内部 image index 作为身份。source metadata 随 `AssetRecord` 保存，用于
去重、保存、重新加载和诊断；不需要额外的 source owner 生命周期。

texture 去重只使用 `AssetSource + TextureColorSpace`。UV 集、材质槽、transform、sampler
和 instance 引用关系不参与图片身份。相同图片以 sRGB 和 Linear 使用时，必须得到两个
`TextureAssetHandle`。

texture 的 CPU ready 与 GPU ready 是不同状态：

```text
CPU: Loading -> Ready | Failed
GPU: Absent -> Uploading -> Published
```

`TextureAssetHandle` 在整个状态转换中保持不变。`TextureRecord::data` 只由
`AssetSystem` 在 CPU completion 事件中写入；GPU 上传由 `RenderAssetSystem` 借用。

## AssetSystem 与 SceneImportHandle

`SceneAssetIngestor` 的状态和逻辑已经并入 `AssetSystem`。当前字段关系如下：

```rust
pub struct AssetSystem {
    store: AssetStore,
    load_service: AssetLoadService,
    scene_imports: SlotMap<SceneImportHandle, SceneImportRecord>,
    scene_loads: HashMap<SceneLoadHandle, SceneImportHandle>,
    texture_loads: HashMap<TextureLoadHandle, TextureAssetHandle>,
}
```

`AssetSystem` 负责：

- 注册 texture、mesh、material；
- 维护 texture 去重；
- 维护 load handle 到 asset/import handle 的映射；
- 校验 `RawSceneData` 并将其转换为 `SceneData`；
- 消费 `AssetLoadEvent`；
- 保存 `SceneImportHandle -> SceneData` 和失败信息。

`AssetSystem` 不接收 `&mut SceneStore`，不创建 `MeshInstanceHandle`，也不创建
`LightHandle`。需要同时检查资源引用和 scene 引用的操作由 `GameWorld` 编排：
`SceneStore` 先检查 live instance，`AssetSystem` 再修改 `AssetStore`。

## SceneData

`SceneData` 是已经完成资源身份解析的 scene 描述，建议只保存资源 handle 和场景对象
描述：

```rust
pub struct SceneData {
    pub scene_path: PathBuf,
    pub objects: Vec<SceneObjectData>,
}

pub struct SceneObjectData {
    pub name: String,
    pub mesh: MeshAssetHandle,
    pub materials: Vec<MaterialAssetHandle>,
    pub transform: glam::Mat4,
}
```

`SceneData` 不保存 `MeshInstanceHandle`、`LightHandle`、GPU slot 或 Vulkan 对象。外部可以
复制或持有 `SceneData`，因为它只包含轻量描述和稳定资源 handle；大块 mesh/texture payload
仍由 `AssetStore` 持有。

`LoadStatus::Ready` 表示 scene 结构、mesh、material 和资源身份已经发布到 CPU
`AssetStore`。它不表示所有 texture 已解码，也不表示 GPU upload、descriptor publish 或
最终画面已经完成。texture 失败可以保留 `SceneData`，由 render-side 使用 fallback。

## 导入流程

### Texture 文件

```text
GameWorld::import_texture(path, color_space)
  -> AssetSystem
  -> canonicalize path
  -> AssetStore 查询 (AssetSource::File, color_space)
  -> 命中：返回 TextureAssetHandle
  -> 未命中：创建 Loading record
              -> AssetLoadService::request_texture
              -> 保存 TextureLoadHandle -> TextureAssetHandle
              -> 返回 TextureAssetHandle

AssetLoadWorker
  -> TextureLoaded(TextureLoadHandle, TextureBytes)
  -> AssetLoadService::update()
  -> AssetSystem
  -> AssetStore::mark_texture_ready()
```

### Mesh 数据

```text
GameWorld::import_mesh(MeshData)
  -> AssetSystem::import_mesh
  -> AssetStore 创建 MeshAssetHandle
  -> 返回 MeshAssetHandle
```

mesh 内容在注册后保持不可变。不同内容创建新 handle；不实现同 handle 热替换。

### Scene 文件

```text
GameWorld::import_scene(path)
  -> AssetSystem 分配 SceneImportHandle
  -> canonicalize scene path，并写入 SceneImportRecord
  -> AssetLoadService::request_scene
  -> 保存 SceneLoadHandle -> SceneImportHandle
  -> 返回 SceneImportHandle

SceneLoaded(SceneLoadHandle, RawSceneData)
  -> AssetSystem 校验 raw scene
  -> 注册 MeshAssetHandle
  -> 将 RawTextureSource 解析为 AssetSource
     Embedded -> { scene_path: canonical_scene_path, image_index }
  -> 必要时请求 TextureLoadHandle
  -> 注册 MaterialAssetHandle
  -> 生成 SceneData
  -> SceneImportHandle 状态变为 Ready
```

外部随后查询 `SceneImportHandle`，根据 `SceneData` 决定哪些对象进入当前
`GameWorld`：

```text
SceneData.objects
  -> GameWorld::create_mesh_instance
  -> SceneStore
  -> MeshInstanceHandle
```

同一个 `SceneData` 可以被过滤、实例化多次，或者只提取其中一部分对象；AssetSystem 不
替外部决定 scene composition。

## GameWorld 对外接口

当前对外 API 为：

```text
import_texture(path, color_space) -> TextureAssetHandle
import_mesh(data) -> MeshAssetHandle
import_scene(path) -> SceneImportHandle

scene_import_status(handle) -> LoadStatus
scene_data(handle) -> Option<&SceneData>

create_mesh_instance(data) -> MeshInstanceHandle
remove_mesh_instance(handle)
register_point_light(...) -> LightHandle
```

`GameWorld` 仍然是 update 阶段的 CPU facade。它可以把 scene 引用检查、asset 删除和
instance 创建组织在一个操作中，但不能把 `SceneStore` 传入 `AssetSystem`。

## 失败、取消和删除

首期继续保持无事务回滚语义：结构校验失败时 `SceneImportHandle` 进入 Failed；在失败前
已经创建但没有引用的资源可以通过普通孤儿资源清理路径删除。若未来需要原子导入，可以
在 `AssetSystem` 内增加临时注册批次，但不能让 SceneStore 重新成为导入器 owner。

首期不增加取消协议。再次导入同一文件创建新的 `SceneImportHandle`；旧任务完成后仍按
自己的 `SceneLoadHandle` 归属原 import。替换场景由外部删除旧的 `MeshInstanceHandle`
并清理无引用资源。

删除边界如下：

- `AssetStore` 检查 material -> texture 等资源内部引用；
- `SceneStore` 检查 mesh/material -> instance、sky 等场景引用；
- `GameWorld` 按顺序组合两类检查；
- CPU 删除不等同于 GPU 立即销毁，render-side 继续按 FIF 和 completion 延迟回收。

重载由 `AssetRecord.source` 驱动：

- `File` source 可以直接重新提交 canonical path；
- `Embedded` source 需要根据 `scene_path + image_index` 重新打开 scene 并提取 encoded image，
  或由 loader 提供等价的 source resolver；
- `Generated` source 只有在 `GeneratedSourceKey` 能重建生成参数时才支持自动重载。

如果同一路径上的文件内容发生变化，`AssetSource` 本身不会自动产生新版本。重载或
invalidation 必须是显式操作，不能把内容 hash 隐式加入当前去重 key。

## 必须保持的不变量

1. loader handle 只属于加载阶段，asset handle 只属于 `AssetStore`，scene object handle 只属于 `SceneStore`。
2. `AssetSystem` 不依赖 `SceneStore`，不创建 `MeshInstanceHandle` 或 `LightHandle`。
3. `SceneData` 不包含 GameWorld 对象身份，也不包含 GPU 身份。
4. 异步 completion 只更新已有 record，不替换资源 handle。
5. CPU ready、GPU installed、frame-visible binding 是独立状态。
6. texture 去重 key 是 `AssetSource + TextureColorSpace`。
7. texture/mesh 内容创建后不可变；内容变化创建新 handle。
8. worker 不修改 CPU owner，不创建 Vulkan 对象。
9. RenderThread、RenderGraph 顺序、GPU 生命周期和 shader ABI 不因本边界调整而改变。

## 边界与非目标

- 本设计不引入全局 asset database、资源包系统、网络复制、事件回放或撤销系统。
- 本设计不把 `AssetSystem` 变成 Vulkan/GPU resource owner，也不合并 `RenderAssetSystem`。
- 本设计不引入跨线程 `GameWorld`、`Arc<RwLock<AssetSystem>>` 或新的通用异步依赖图。
- 首期不实现 texture/mesh 内容热替换；内容变化通过新 asset handle 表达。
- source 使用扁平值描述，不引入 `SourceAssetId`、source registry 或 source owner 生命周期。
- 首期不增加 scene import 取消协议和事务回滚；无引用的中途资源沿用孤儿清理路径。
- 本设计不改变 `RenderRuntime::prepare`、RenderGraph pass 顺序、FIF 回收或 shader ABI。

## 完成标准

- loader、asset、scene object 三类身份在 public API 和模块文档中不再混用。
- `AssetSystem` 可以独立消费 load event，并且不需要 `SceneStore` 参数。
- 相同 `AssetSource + TextureColorSpace` 只产生一个 live `TextureAssetHandle`；不同颜色空间产生不同 handle。
- `SceneImportHandle` 可以查询 `Loading`、`Ready` 或 `Failed`，`Ready` 时可获得包含 asset handle 的 `SceneData`。
- 导入完成不会自动创建 `MeshInstanceHandle`；外部可以选择性创建、重复实例化或过滤 scene objects。
- texture CPU completion、GPU upload、descriptor publish 和最终画面仍能通过独立状态与日志区分。
- 现有 RenderWorld、GPU resource manager 和 shutdown 生命周期不因这次边界迁移而失去 owner。

## 与现有设计的关系

- [`scene-import-and-async-texture.md`](scene-import-and-async-texture.md) 负责 raw scene、
  external/embedded texture source 和异步解码输入；本文负责长期 asset identity、scene
  import 出口和 GameWorld 边界。
- [`asset-upload-and-scene-evolution.md`](asset-upload-and-scene-evolution.md) 负责上传调度、
  失败恢复、资源卸载和 GPU retirement；本文不重新定义 GPU owner。
- [`../summaries/scene-data-lifecycle.md`](../summaries/scene-data-lifecycle.md) 记录完成实施
  后的 CPU/GPU 当前事实；本文保留跨模块设计不变量和演进边界，不替代运行时证据。

## 已实施与后续演进

本轮已完成加载服务/worker 和 handle 重命名、`AssetSystem` 合并、扁平 source 去重、
`TextureRecord`、`SceneData` 出口以及 Renderer 的显式 instance 创建；GPU owner、
`RenderRuntime::prepare` 顺序和 FIF 回收边界保持不变。

后续若增加显式重载、Generated source resolver、导入事务或取消协议，应继续按 owner 和
行为边界拆分提交，不在行为变化中混入无关格式化或文件移动。
