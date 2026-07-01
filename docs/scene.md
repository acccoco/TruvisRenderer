# Scene 资产与 GPU 同步设计

> 状态：目标设计说明。本文记录后续 `SceneStore` / `AssetHub` / `RenderWorld` 内部 render managers
> 的职责边界，重点说明 texture/model 资产加载、CPU scene 语义变化、GPU dirty 路由、
> sky / material / instance buffer 上传和 TLAS 构建的完整过程。
> 代码重构不保留双 handle 兼容层：`SceneStore`、live `Instance`、raycast hit 和
> `RenderWorld` manager 的长期引用使用本文定义的 CPU resource handle owner 边界。`AssetHub`
> 内部 handle 只允许作为 loader / ingest 私有状态存在。

## 总体原则

- `AssetHub` 是 loader，不是长期 asset database。
- `SceneStore` 是 CPU 语义世界的 owner，保存可查询、可编辑的 scene 资源；v1 不设计 scene
  persistence / stable saved id。
- CPU resource handle 只作为运行时 SlotMap handle，不作为磁盘序列化 ID 或跨进程稳定 ID。
- `RenderRuntime` 持有 `World` 和 `RenderWorld`；`World` 是 App-facing CPU semantic world / asset facade，
  `RenderWorld` 是 render-side prepared world / GPU cache owner。
- `RenderRuntime` 通过 `RenderWorld` 拥有 GPU 派生状态和缓存，负责 texture / mesh upload、bindless
  注册、material / instance buffer 上传、TLAS 构建和延迟释放。
- `RenderWorld` 内部持有所有 `RenderXXXManager`、sky manager 和 light table owner；dirty routing 由
  `DirtyRouterHelper` 作为 prepare helper 通过 event / rule / command 转换完成，不作为 `RenderWorld` 字段。
- `SceneStore` 不保存 Vulkan image / image view / bindless handle / material slot / GPU ready 状态。
- `RenderRuntime` 不反向拥有 CPU scene 语义；它在 prepare 阶段读取 scene 快照并更新 GPU 可见缓存。

```text
RenderRuntime
  world: World
    scene: SceneStore
      TextureHandle / MaterialHandle / MeshHandle / InstanceHandle
      SceneChanges + reverse dependency indices
    scene_assets: SceneAssetIngestor
      ModelImportHandle
      PendingTextureUpload(TextureHandle, revision, TextureCpuData)
      PendingSkyDistributionUpload(TextureHandle, texture_revision, sky_revision, SkyDistributionCpuData)
      PendingMeshUpload(MeshHandle, revision, MeshCpuData)
    assets: AssetHub
      TextureLoadHandle / ModelLoadHandle
    World::sync_for_render() -> WorldRenderSync
  render_world: RenderWorld
    RenderTextureManager / RenderMeshManager / RenderMaterialManager
    RenderInstanceManager / RenderTlasManager
    RenderSkyManager / RenderAnalyticLightManager / RenderEmissiveLightTable
    DirtyRouterHelper (stateless prepare helper, not a field)
    scene root buffer / raster draw cache / RenderSceneView
```

## 模块与 owner 归属

| 对象 | 建议模块 | Owner / 内嵌关系 |
| --- | --- | --- |
| `World` | `truvis-world` | `RenderRuntime.world`，App-facing CPU semantic world / asset facade |
| `SceneStore` / CPU resource handle / `SceneChanges` | `truvis-world` | `World.scene` 私有 owner；跨 crate 只暴露只读 `SceneReadView` |
| `TextureSource` / `MeshSource` / `ModelSource` / import desc | `truvis-asset` 公共 source / import 模块 | asset-neutral 类型；可被 `AssetHub` 和 `SceneStore` 共同使用，不包含 CPU resource handle |
| `AssetHub` | `truvis-asset` | `World.assets` |
| `SceneAssetIngestor` | `truvis-world` | `World.scene_assets`，不内嵌进 `SceneStore` |
| `RenderWorld` | `truvis-render-runtime::render_world` | `RenderRuntime.render_world` |
| `RenderTextureManager` / `RenderMeshManager` / `RenderMaterialManager` | `truvis-render-runtime::render_world` 私有子模块 | `RenderWorld` 私有字段 |
| `RenderInstanceManager` / `RenderTlasManager` | `truvis-render-runtime::render_world` 私有子模块 | `RenderWorld` 私有字段 |
| `RenderSkyManager` / `RenderAnalyticLightManager` / `RenderEmissiveLightTable` | `truvis-render-runtime::render_world` 私有子模块 | `RenderWorld` 私有字段 |
| `DirtyRouterHelper` | `truvis-render-runtime::render_world` 私有 helper 模块 | `RenderWorld::prepare` 使用的 stateless helper，不是字段 |
| `RenderSceneView` / `RenderSceneAccumSignature` | `truvis-render-foundation::render_scene_view` | `RenderWorld` 实现，render pass 只依赖该只读 trait |

`RenderWorld` 是 render-side prepared world 和 GPU cache owner 聚合体。render pass 不能直接访问内部
`RenderXXXManager`；它只能通过 `RenderSceneView` 获取 scene root buffer、TLAS handle 和 raster draw 能力。

## 主要对象与职责

### `World`

`World` 是 App 在 update 阶段面对的 CPU semantic world / asset facade。App 不直接访问 `SceneStore`、
`AssetHub` 或 `SceneAssetIngestor` 的实现细节，而是通过 `World` 的窄接口表达导入和编辑意图。

目标字段形状：

```rust
pub struct World {
    scene: SceneStore,
    assets: AssetHub,
    scene_assets: SceneAssetIngestor,
}
```

`SceneStore` 是 `World` 的私有 owner，不作为 App 或 render runtime 的构造依赖暴露。
render-side prepare 如需读取 CPU scene，只通过 `World::scene_view() -> SceneReadView<'_>` 获取只读快照。
`SceneReadView` 不提供编辑接口，也不拥有 loader handle、CPU bytes 或 GPU resource。

目标接口：

```rust
impl World {
    pub fn new() -> Self;

    pub fn request_model_import(
        &mut self,
        path: PathBuf,
        import_desc: ModelImportDesc,
    ) -> ModelImportHandle;

    pub fn model_import_status(
        &self,
        handle: ModelImportHandle,
    ) -> SceneModelImportStatus;

    pub fn scene_view(&self) -> SceneReadView<'_>;

    pub fn register_texture(
        &mut self,
        path: PathBuf,
        import: TextureImportDesc,
    ) -> Result<TextureHandle, WorldEditError>;

    pub fn register_texture_data(
        &mut self,
        source: TextureSource,
        import: TextureImportDesc,
        data: TextureCpuData,
    ) -> Result<TextureHandle, WorldEditError>;

    pub fn remove_texture(
        &mut self,
        texture: TextureHandle,
    ) -> Result<(), WorldEditError>;

    pub fn register_material(
        &mut self,
        desc: SceneMaterialDesc,
    ) -> Result<MaterialHandle, WorldEditError>;

    pub fn update_material(
        &mut self,
        material: MaterialHandle,
        update: SceneMaterialUpdate,
    ) -> Result<(), WorldEditError>;

    pub fn remove_material(
        &mut self,
        material: MaterialHandle,
    ) -> Result<(), WorldEditError>;

    pub fn register_mesh(
        &mut self,
        source: MeshSource,
        import: MeshImportDesc,
        data: MeshCpuData,
    ) -> Result<MeshHandle, WorldEditError>;

    pub fn remove_mesh(
        &mut self,
        mesh: MeshHandle,
    ) -> Result<(), WorldEditError>;

    pub fn register_instance(
        &mut self,
        desc: SceneInstanceDesc,
    ) -> Result<InstanceHandle, WorldEditError>;

    pub fn update_instance_materials(
        &mut self,
        instance: InstanceHandle,
        materials: Vec<MaterialHandle>,
    ) -> Result<(), WorldEditError>;

    pub fn update_instance_transform(
        &mut self,
        instance: InstanceHandle,
        transform: Affine3A,
    ) -> Result<(), WorldEditError>;

    pub fn remove_instance(
        &mut self,
        instance: InstanceHandle,
    ) -> Result<(), WorldEditError>;

    pub fn update_sky(
        &mut self,
        update: SceneSkyUpdate,
    ) -> Result<(), WorldEditError>;

    pub fn register_light(
        &mut self,
        desc: SceneAnalyticLightDesc,
    ) -> Result<LightHandle, WorldEditError>;

    pub fn update_light(
        &mut self,
        light: LightHandle,
        update: SceneAnalyticLightUpdate,
    ) -> Result<(), WorldEditError>;

    pub fn remove_light(
        &mut self,
        light: LightHandle,
    ) -> Result<(), WorldEditError>;

    pub fn sync_for_render(&mut self) -> WorldRenderSync;
}
```

这些 API 是 App-facing facade；App 不应直接拿到 `SceneStore` 的可变引用，也不应直接调用
`SceneAssetIngestor`。`register_texture_data` / `register_mesh` 代表 runtime / procedural 数据直接进入
scene 的路径：`World` 先在 `SceneStore` 注册 metadata，再把 `TextureCpuData` / `MeshCpuData` 放入
`SceneAssetIngestor` 的短期 pending upload 队列。文件 texture 则由 `World::register_texture` 注册
`TextureHandle` 后交给 `SceneAssetIngestor` 提交 loader 请求。

`World::sync_for_render()` 在 App update 之后、`RenderWorld.prepare(...)` 之前执行。它提交尚未发送的
asset load request，drain `AssetHub` 的完成事件，把成功的 model import 原子提交到 `SceneStore`，
drain `SceneStore` 的 CPU 语义 change log，并输出 render-side prepare 需要的同步包：

```rust
pub struct WorldRenderSync {
    pub scene_changes: SceneChanges,
    pub asset_uploads: SceneAssetSyncOutput,
}

pub struct SceneAssetSyncOutput {
    pub pending_texture_uploads: Vec<PendingTextureUpload>,
    pub pending_sky_distribution_uploads: Vec<PendingSkyDistributionUpload>,
    pub pending_mesh_uploads: Vec<PendingMeshUpload>,
}
```

`SceneStore::drain_changes()` 只由 `World::sync_for_render()` 调用。`RenderWorld.prepare(...)`
不直接 drain CPU scene change log，而是消费 `WorldRenderSync.scene_changes` 和
`WorldRenderSync.asset_uploads`。

`World::request_model_import(path, import_desc)` 在提交给 `SceneAssetIngestor` 前先对 model 主路径执行
filesystem canonicalize。canonicalize 成功后的 path 进入 `ModelLoadDesc` 和 `ModelSource`；失败时创建
`ModelImportHandle` 并直接标记为 `SceneModelImportStatus::Failed(error)`，不向 `AssetHub` 提交
loader request。

edit error 也需要保持 facade 边界清晰。`SceneStore` 内部 edit API 返回 `SceneEditError`；
`World` 对外返回 `WorldEditError`，用于补充 filesystem canonicalize、asset ingest request 创建等
facade 层错误：

```rust
pub enum WorldEditError {
    Scene(SceneEditError),
    FilesystemCanonicalizeFailed { path: PathBuf, error: std::io::Error },
    InvalidAssetRequest { reason: &'static str },
}

pub enum SceneEditError {
    StaleHandle { kind: SceneHandleKind },
    MissingDependency { kind: SceneHandleKind },
    StillReferenced { kind: SceneHandleKind, dependents: SceneDependents },
    MaterialSubmeshCountMismatch { expected: usize, actual: usize },
    DuplicateOrInvalidOperation { reason: &'static str },
}
```

所有失败的 edit 都必须保持事务语义：不推进 revision、不写入 `SceneChanges`、不修改反向依赖索引。
如果实现中保留更细的 `SceneDeleteError::StillReferenced { dependents }`，它应作为
`SceneEditError::StillReferenced` 的专用来源或等价别名，而不是形成另一套删除规则。

### `SceneStore`

`SceneStore` 持有 scene 内长期存在的 CPU 语义对象。它只关心 scene handle，不直接持有 loader handle。
目标 API 不兼容旧 asset-side handle 作为长期 scene 引用的模式；
`SceneStore`、`SceneInstance` 和各 `RenderXXXManager` 的目标接口只使用 CPU resource handle。

维护状态：

- `TextureHandle -> SceneTexture`
- `MaterialHandle -> SceneMaterial`
- `MeshHandle -> SceneMesh`
- `InstanceHandle -> SceneInstance`
- `LightHandle -> SceneAnalyticLight`
- `SceneSkyState`，记录 sky / environment 的 CPU 权威状态
- `SceneTextureKey -> TextureHandle` 的去重表
- `TextureHandle -> Vec<MaterialHandle>` 的内部反向依赖索引
- sky texture 依赖查询：当前 sky 是否引用某个 `TextureHandle`
- `MaterialHandle -> Vec<InstanceHandle>` 的内部反向依赖索引
- `MeshHandle -> Vec<InstanceHandle>` 的内部反向依赖索引
- scene 资源 revision，用于 render-side 判断哪些 GPU 缓存需要更新
- CPU 语义变化日志：`SceneChanges`

`SceneTextureKey` 是 scene 级 texture identity，同一 key 永远只对应一个 `TextureHandle`：

```rust
pub struct SceneTextureKey {
    pub source: TextureSource,
    pub import: TextureImportDesc,
}
```

source identity 明确区分文件、procedural 和 runtime 资源：

```rust
pub enum TextureSource {
    File { canonical_path: PathBuf },
    Procedural { id: ProceduralTextureId },
    Runtime { id: RuntimeTextureId },
}

pub enum MeshSource {
    Model {
        model: ModelSource,
        mesh_index: u32,
    },
    Procedural { id: ProceduralMeshId },
    Runtime { id: RuntimeMeshId },
}

pub enum ModelSource {
    File { canonical_path: PathBuf },
}
```

filesystem canonicalize 只适用于 `File` source。canonical path 是文件资源 identity 的一部分；
canonicalize 失败不降级为 lexical normalize，而是让对应 texture 注册或 model ingest 失败。
`Procedural` / `Runtime` source 的 id 由创建方保证唯一，不走 filesystem canonicalize。
这些 source / import desc 是 asset-neutral 类型，目标上应定义在 `truvis-asset` 的公共 source / import
模块中，供 `AssetHub` 和 `SceneStore` 共同引用；它们不得包含 CPU resource handle，避免 `truvis-asset`
反向依赖 `truvis-world`。

`SceneTextureKey` 一经注册不可 re-key。修改 path、color space、decode options 或 mip 策略不应改变
已有 `TextureHandle` 的 key；调用方应注册或复用另一个 `TextureHandle`，再更新 material
texture slot 或 sky/environment 引用。旧 texture handle 按普通依赖检查删除，不能通过 re-key 复活或变形。

CPU resource handle 都是 `SceneStore` 内部 SlotMap key。删除后旧 handle 不应再查询到值；
`SceneStore::get_*` 对 stale handle 返回 `None`，更新 stale handle 返回失败，不创建 tombstone，
也不允许旧 handle 复活为新资源。render-side stable slot 与 CPU resource handle 是不同身份：
GPU slot 可延迟回收，但 CPU handle 的 live / stale 语义只由 `SceneStore` 的 SlotMap 决定。

删除 texture / material / mesh 前必须先通过 `SceneStore` 的反向依赖索引检查依赖关系。存在依赖时，
删除请求失败，不删除资源、不推进 revision、不写入 change log：

```rust
pub enum SceneDeleteError {
    StillReferenced {
        dependents: Vec<SceneHandleRef>,
    },
    StaleHandle,
}

pub enum SceneHandleRef {
    Texture(TextureHandle),
    Mesh(MeshHandle),
    Material(MaterialHandle),
    Instance(InstanceHandle),
    SkyEnvironment,
}
```

具体规则是：删除 texture 时 `texture -> material` 必须为空，且当前 sky 不得引用该 texture；
删除 material 时 `material -> instance` 必须为空；删除 mesh 时 `mesh -> instance` 必须为空。
删除 instance 不需要依赖检查，直接允许。

`SceneSkyState` 维护 sky / environment 的 CPU 权威语义状态：

```rust
pub struct SceneSkyState {
    pub enabled: bool,
    pub intensity: f32,
    pub texture: Option<TextureHandle>,
    pub revision: u64,
}
```

v1 只把 enabled、亮度、texture handle 和 revision 纳入目标设计；rotation、tint、多 sky 和 procedural sky
暂不展开。`SceneSkyState` 不保存 sky alias table、GPU binding 或 `TextureCpuData`。`SceneStore`
在修改 sky texture 时必须同步维护 sky 对 texture 的依赖查询；删除被 sky 引用的 texture 时，依赖列表应返回
`SceneHandleRef::SkyEnvironment`。

`SceneTexture` 维护 CPU 侧 texture 语义、加载状态和 scene revision，但不长期保存 CPU bytes：

```rust
pub struct SceneTexture {
    pub source: TextureSource,
    pub import: TextureImportDesc,
    pub cpu_status: SceneTextureCpuStatus,
    pub revision: u64,
}
```

其中 `TextureImportDesc` 保存 texture 自身的导入语义，例如 color space、是否生成 mip、解码格式意图等。
`TextureCpuData` 只作为加载完成事件和短期上传 payload 流动；上传提交后即可释放，不属于
`SceneStore` 的长期语义状态。texture 到 material 的反向依赖索引由 material 注册或更新时维护，
只用于 CPU scene 查询和 render-side dirty 推导，不表示 GPU 状态。

`SceneMaterial` 维护材质槽位语义：

```rust
pub struct SceneMaterial {
    pub name: String,
    pub base_color: glam::Vec4,
    pub emissive: glam::Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub opaque: f32,
    pub diffuse_texture: Option<TextureHandle>,
    pub normal_texture: Option<TextureHandle>,
    pub revision: u64,
}
```

`diffuse_texture`、`normal_texture` 这类字段表达的是材质槽位用途，应放在 `SceneMaterial`；
`Srgb` / `Linear` 这类导入解释方式应放在 `SceneTexture.import`。

`SceneStore` 为 emissive table 提供 CPU 权威材质参数 resolver。该 resolver 只能借用 scene store
中的轻量材质参数，不 clone 大块数据，也不查询 GPU slot：

```rust
pub struct SceneMaterialEmissiveParams<'a> {
    pub material: MaterialHandle,
    pub base_color: &'a glam::Vec4,
    pub emissive: &'a glam::Vec4,
    pub opaque: f32,
    pub revision: u64,
}

pub trait SceneMaterialEmissiveResolver<'a> {
    fn get(&self, material: MaterialHandle) -> Option<SceneMaterialEmissiveParams<'a>>;
}
```

`SceneMesh` 维护 mesh 的长期语义 metadata，但不长期保存 vertex / index CPU bytes。`SceneMesh`
是 submesh 列表 owner；一个 submesh 对应一个完整 geometry，拥有自己的 vertex / index 数据和 AABB：

```rust
pub struct SceneMesh {
    pub source: MeshSource,
    pub import: MeshImportDesc,
    pub name: String,
    pub submeshes: Vec<SceneSubmesh>,
    pub local_aabb: Aabb,
    pub cpu_status: SceneMeshCpuStatus,
    pub revision: u64,
}
```

```rust
pub struct SceneSubmesh {
    pub name: String,
    pub local_aabb: Aabb,
}
```

一个 `SceneInstance` 只能引用一个 `MeshHandle`，但可以有多个 material。`SceneInstance.materials.len()`
必须等于 `SceneMesh.submeshes.len()`；第 `i` 个 material 对应第 `i` 个 submesh / geometry。更新 instance
mesh 或 material list 时必须重新验证这个长度约束，并同步维护 `material -> instance` 与 `mesh -> instance`
反向依赖。

`MeshCpuData` 只作为 `ModelCpuData`、`PendingMeshUpload` 或其他短期 upload payload 流动：

```rust
pub struct MeshCpuData {
    pub submeshes: Vec<SubmeshCpuData>,
}

pub struct SubmeshCpuData {
    pub positions: Vec<glam::Vec3>,
    pub normals: Vec<glam::Vec3>,
    pub tangents: Vec<glam::Vec3>,
    pub uvs: Vec<glam::Vec2>,
    pub indices: Vec<u32>,
}
```

提交给 `RenderMeshManager` 后即可释放，不属于 `SceneStore` 或 `RenderMeshManager` 的长期状态。
如果未来 emissive table 需要 CPU triangle metadata，应单独定义 derived metadata owner；本设计不让
`RenderMeshManager` 保存完整 mesh CPU 数据。

`SceneStore` 的 revision 和 change log 只表达 CPU 语义变化，不表达 GPU upload dirty 或 GPU ready。
调用方添加 / 更新 material、添加 / 更新 instance、删除 scene resource 或更新 sky / light 等状态时，
`SceneStore` 推进对应资源 revision，
并把变化记录到 `SceneChanges`。`World::sync_for_render()` drain 这些变化并放入 `WorldRenderSync`，
随后 `RenderWorld::prepare` 通过 `DirtyRouterHelper` 转成 dirty command，再 apply 到 render-side manager：

```rust
pub struct SceneChanges {
    pub removed_textures: Vec<TextureHandle>,
    pub removed_meshes: Vec<MeshHandle>,
    pub changed_materials: Vec<MaterialHandle>,
    pub removed_materials: Vec<MaterialHandle>,
    pub changed_instances: Vec<SceneInstanceChange>,
    pub removed_instances: Vec<InstanceHandle>,
    pub changed_analytic_lights: bool,
    pub changed_sky_environment: bool,
}

pub struct SceneInstanceChange {
    pub instance: InstanceHandle,
    pub kind: SceneInstanceChangeKind,
}

pub enum SceneInstanceChangeKind {
    Lifecycle,
    MaterialBinding,
    Transform,
}
```

`SceneStore::drain_changes() -> SceneChanges` 返回并清空这一帧累积的 CPU 语义变化。反向依赖索引
同样只属于 CPU scene 查询和 dirty 推导，不属于 GPU 派生状态。

`SceneChanges` 是合并后的 change log，而不是逐操作 append-only 日志。`SceneStore`
内部使用 map / set 累积变化，`drain_changes()` 时再输出 vec。推荐内部结构是：

- `removed_textures` 使用 `HashSet<TextureHandle>`
- `removed_meshes` 使用 `HashSet<MeshHandle>`
- `changed_materials` / `removed_materials` 使用 `HashSet<MaterialHandle>`
- `changed_instances` 使用 `HashMap<InstanceHandle, SceneInstanceChangeKind>`
- `removed_instances` 使用 `HashSet<InstanceHandle>`
- analytic light 使用布尔 dirty flag
- sky / environment state 使用布尔 dirty flag

合并规则：

- 同一 resource 的 `removed_*` 强于 `changed_*`；资源已删除时不再输出 changed。
- 同一个 instance 的 dirty kind 按强度保留最强项：`Lifecycle > MaterialBinding > Transform`。
- 同帧 create 后 delete 且从未被 render-side 观察到时可以合并为 no-op；否则输出 removed，由对应
  render manager 查询已有 GPU binding 后决定是否回收。
- texture / material / mesh / instance 删除时，`SceneStore` 必须同步清理反向依赖索引，避免
  `DirtyRouterHelper` 后续路由 stale dependent handle。

`SceneStore` 的 stores 不应向外暴露可变引用；所有 scene 语义修改都通过内部
`register_*`、`update_*` 和 `remove_*` 方法进入，并返回 `Result<_, SceneEditError>`。
这些方法由 `World` facade 或 `SceneAssetIngestor` 调用，不作为 App-facing API 暴露：

- `register_texture` / `register_mesh` 负责分配 SlotMap handle、初始化 revision、建立 metadata / key；
  texture / mesh 添加不写入 `SceneChanges`，render-side 上传由 `PendingTextureUpload` /
  `PendingMeshUpload` 驱动。
- runtime / procedural texture 或 mesh 的 CPU bytes 不进入 `SceneStore`；`World` 只把 metadata 注册到
  `SceneStore`，然后把 bytes 放入 `SceneAssetIngestor` 的短期 pending upload 队列。
- `register_material` / `register_instance` 负责分配 SlotMap handle、初始化 revision、写入 change log，
  并建立需要的反向依赖。
- `update_material_texture_slot` 必须同时更新 texture -> material 反向依赖、material revision 和
  `SceneChanges.changed_materials`。
- `update_instance_materials` 必须验证 mesh submesh 数量与 material list 对齐，同步更新
  material -> instance 反向依赖、instance revision 和最强 `SceneInstanceChangeKind::MaterialBinding`。
- instance 引用的 mesh 创建后不更新；如果需要改变 mesh，应删除旧 instance 并创建新 instance。
- `update_instance_transform` 只推进 instance revision，并写入 `Transform` dirty；如果同一帧已经存在
  更强 dirty kind，则保留更强项。
- `update_sky_*` 只修改 `SceneSkyState` 的 enabled、intensity 或 `TextureHandle` 引用，推进
  sky/environment revision，并写入 `changed_sky_environment`；更新 texture 引用时必须同步维护 sky texture
  依赖。sky runtime owner 不能绕过该 handle 直接请求 `AssetHub`。
- `remove_*` 必须先执行依赖检查；失败时不推进 revision、不写 change log。create 后同帧 delete 且尚未
  `drain_changes()` 输出时合并为 no-op；已被 drain 观察后的删除输出 removed。

### `AssetHub`

`AssetHub` 只负责异步读取和 CPU 解码，不长期持有 scene 材质、mesh 或 texture 语义。
它是一次性 loader service，不负责跨 scene 或跨调用方的长期 asset identity / loader request 去重。

维护状态：

- in-flight `TextureLoadHandle -> TextureLoadRecord`
- in-flight `ModelLoadHandle -> ModelLoadRecord`
- 后台任务队列和完成事件队列
- loader 状态：`Unloaded` / `Loading`

`TextureLoadDesc` 表示本次 texture loader task 如何读取 / decode CPU texture data，
至少应包含：

```rust
pub struct TextureLoadDesc {
    pub source: TextureSource,
    pub color_space: TextureColorSpace,
    pub decode: TextureDecodeOptions,
}

pub struct ModelLoadDesc {
    pub source: ModelSource,
    pub import: ModelImportDesc,
}

pub struct TextureLoadRecord {
    pub desc: TextureLoadDesc,
    pub state: AssetLoadTaskState,
}

pub struct ModelLoadRecord {
    pub desc: ModelLoadDesc,
    pub state: AssetLoadTaskState,
}
```

`diffuse` / `normal` 这类 material 槽位用途不进入 `TextureLoadDesc`；importer 可以根据用途推导默认
`color_space` 或 decode options，但注册后的 scene identity 只由 `SceneTextureKey = TextureSource + TextureImportDesc`
决定。如果同一路径因为 color space、decode options 或 mip 策略不同而产生不同 CPU/GPU 数据，
应由 `SceneStore` 生成不同 `SceneTextureKey` / `TextureHandle`。`File` source 在进入 desc 前必须已经
canonicalize；`Procedural` / `Runtime` source 使用其 id 作为 identity，不走 filesystem canonicalize。
`AssetHub` 不维护 `TextureLoadDesc -> TextureLoadHandle` 或 `ModelLoadDesc -> ModelLoadHandle` 去重表；
同一 scene texture 只提交一次 loader request 由 `SceneStore` / `SceneAssetIngestor` 保证。

sampler 不属于 `SceneTextureKey` 或 `TextureLoadDesc`。SRV 与 sampler 在 shader-visible binding 中分离：
texture key 只描述需要生成哪份 texture 数据；采样方式由 material 槽位参数、runtime 默认策略或环境绑定
决定。即使两个材质以不同 sampler 使用同一张 image，也应复用同一个 `TextureHandle`。

v1 不支持 per-material sampler。material texture 沿用当前 runtime 默认策略：普通材质贴图使用
`LinearRepeat`，sky / environment 使用 `LinearClamp`；sampler 由 render-side resolver / binding 写入
shader-visible material 或 environment binding，不进入 `SceneTextureKey`。

texture 完成事件只携带 loader handle、desc 和 CPU bytes：

```rust
pub enum TextureLoadEvent {
    Ready {
        handle: TextureLoadHandle,
        desc: TextureLoadDesc,
        data: TextureCpuData,
    },
    Failed {
        handle: TextureLoadHandle,
        desc: TextureLoadDesc,
        error: String,
    },
}
```

任务完成并生成 `TextureLoadEvent` 后，`AssetHub` 应立即移除对应的
`TextureLoadHandle -> TextureLoadRecord`。event 自身拥有 `TextureCpuData`，调用方不能通过完成后的 handle
再回查 `AssetHub` 获取 bytes。model task 遵循同一规则：生成 `ModelLoadEvent` 后移除
`ModelLoadHandle -> ModelLoadRecord`，event 自身拥有 `ModelCpuData`。

### `SceneAssetIngestor`

`SceneAssetIngestor` 是 `World` 内部的 scene asset ingest pipeline，负责把 `AssetHub` event
写入 `SceneStore`，并生成短期 render upload payload。它作为 `World` 的内部成员存在，
但不应让 `SceneStore` 直接依赖 `AssetHub`，也不应让 `AssetHub` 直接依赖 `SceneStore`。

维护状态：

- pending texture load queue：`TextureHandle + TextureLoadDesc + SceneTextureLoadPurpose`
- submitted texture load table：`TextureLoadHandle -> SubmittedTextureLoad`
- scene texture 的加载请求是否已经提交，避免同一 `TextureHandle` 重复提交 loader task
- `ModelImportHandle -> ModelLoadHandle`
- `ModelLoadHandle -> ModelImportHandle`
- scene model import 的加载请求、导入状态和错误信息
- 短期上传收件箱：`PendingTextureUpload { scene_texture, revision, data }`
- 短期上传收件箱：`PendingSkyDistributionUpload { scene_texture, texture_revision, sky_revision, data }`
- 短期上传收件箱：`PendingMeshUpload { scene_mesh, revision, data }`

```rust
pub enum SceneTextureLoadPurpose {
    TextureUpload,
    SkyDistributionOnly,
    TextureUploadAndSkyDistribution,
}

pub struct SubmittedTextureLoad {
    pub scene_texture: TextureHandle,
    pub texture_revision: SceneRevision,
    pub purpose: SceneTextureLoadPurpose,
}
```

`SceneStore` 保证同一 `SceneTextureKey` 只有一个 `TextureHandle`，因此 `SceneAssetIngestor`
只需要维护 scene texture 到 loader task 的一对一等待关系。`SceneTextureLoadPurpose` 只属于
`SceneAssetIngestor`，用于决定 loader event 到达后生成 texture upload、sky distribution upload 或两者；
`AssetHub` 仍只看到 `TextureLoadDesc` 和 `TextureLoadHandle`，不知道 scene handle、sky 目的或 render upload 目的。
`AssetHub` 不做 desc 级 in-flight 去重；加载完成后，协调层根据 load handle 等待表更新对应 scene texture 的 CPU 状态和 revision，并把 event 中的
`TextureCpuData` 转成短期
`PendingTextureUpload`。该 payload 只保留到 `RenderTextureManager` 提交上传；提交后立即释放。
texture 添加和 CPU load ready 不写入 `SceneChanges`；render-side texture 创建 / 替换由
`PendingTextureUpload` 和 `RenderTextureUpdateResult.ready_changed_textures` 驱动。
sky texture 也必须先通过 `World` / `SceneStore` 注册为 `TextureHandle`，不由 runtime 私有的
sky / environment owner 直接请求 `AssetHub` texture。

如果加载完成的 texture 当前被 `SceneSkyState` 引用，`SceneAssetIngestor` 应在移动 `TextureCpuData`
进入 `PendingTextureUpload` 前，借用该 CPU bytes 派生 `SkyDistributionCpuData`，并生成
`PendingSkyDistributionUpload`。该 payload 只保存 alias table / 分布构建结果等轻量数据，不复制或长期保存
整张 HDRI bytes；提交给 `RenderSkyManager` 上传后即可释放。

如果 sky 后续切换到一个已经 GPU 上传完成、但当前没有 sky distribution 的 texture，`SceneAssetIngestor`
可以重新发起一次 CPU texture load，并以 `SceneTextureLoadPurpose::SkyDistributionOnly` 记录本次 load。
该 distribution-only load 完成后仍只产生短期 `PendingSkyDistributionUpload`；失败时由
`RenderSkyManager` 使用 fallback distribution，不使 scene import 失败。

对于 runtime / procedural texture 或 mesh，`World` 不需要经过 `AssetHub`：它先把 scene metadata 注册到
`SceneStore`，再把 `TextureCpuData` / `MeshCpuData` 直接压入 `SceneAssetIngestor` 的短期 pending upload
收件箱。这样 texture / mesh 添加仍然不进入 `SceneChanges`，也不会要求 `AssetHub` 成为长期 asset database。

如果 loader event 到达时等待表中已经没有对应 scene handle，或对应 CPU resource handle 在 `SceneStore`
中已经 stale，`SceneAssetIngestor` 应直接丢弃 event payload：`TextureCpuData` / `MeshCpuData` /
`ModelCpuData` 不进入 pending upload，不重新创建 scene 资源，也不触发重试。这保证删除 scene 资源后，
迟到的后台加载结果不会把旧资源“复活”。

App 请求导入 FBX / glTF 等 model 时，应调用 `World::request_model_import(path, import_desc)` 并获得
`ModelImportHandle`。`World` 内部转发给 `SceneAssetIngestor`，后者再向 `AssetHub.request_model(desc)`
提交实际 loader 请求，记录 `ModelImportHandle` 与 `ModelLoadHandle` 的映射。App 可以通过
`World::model_import_status(handle)` 或 scene 查询显示导入进度，但不应直接访问 `SceneAssetIngestor`
或轮询 `AssetHub` 的 model handle。

`ModelLoadHandle` 只表示一次 in-flight model loader task。任务完成后，`AssetHub` 通过
`ModelLoadEvent` 交付完整 CPU 导入包；`SceneAssetIngestor` 在 asset sync 阶段 drain 这些 event，
成功时构造并验证 `ModelImportPlan`，再调用
`SceneStore::import_model_transaction(plan) -> Result<ModelImportResult, ImportError>` 原子写入
scene；失败时写入对应 import handle 的错误状态，不留下半注册 scene。

### `RenderWorld`

`RenderWorld` 是 render-side prepared world 和 GPU cache owner 聚合体，由 `RenderRuntime` 持有。它内部聚合
render managers、sky manager、analytic / emissive light owners、scene root buffer、raster draw cache 和 TLAS view；render pass
只通过 `RenderSceneView` 读取 `RenderWorld` 的只读快照。

目标字段形状：

```rust
pub struct RenderWorld {
    render_texture_manager: RenderTextureManager,
    render_mesh_manager: RenderMeshManager,
    render_material_manager: RenderMaterialManager,
    render_instance_manager: RenderInstanceManager,
    render_tlas_manager: RenderTlasManager,
    render_sky_manager: RenderSkyManager,
    render_analytic_light_manager: RenderAnalyticLightManager,
    render_emissive_light_table: RenderEmissiveLightTable,
}
```

`RenderWorld::prepare(...)` 或等价内部流程负责按固定顺序推进内部 managers。
`DirtyRouterHelper` 是集中 dirty routing helper，但不是 `RenderWorld` 字段或 owner；`RenderWorld::prepare`
消费 `WorldRenderSync` 中的 `SceneChanges` 和短期 upload payload，并在各 `RenderXXXManager.update()` 和 light table update
之间把 scene change / update result 转成 `DirtyEvent`，通过静态 rule set 生成 `DirtyCommand`，
再把命令 apply 到下一步 owner。

`RenderRuntime::begin_frame` 应调用 `RenderWorld::begin_frame(frame_token)`；`RenderWorld::begin_frame`
再转发给各 `RenderXXXManager` / sky manager / light owner 推进 frame token、回收跨 FIF 的 retired slot 或 deferred
resource。`RenderWorld::prepare` 只消费当前 `FrameLabel`，不推进 frame id，也不承担跨 FIF 回收窗口计时。

推荐顺序：

```text
changes = sync.scene_changes
asset_uploads = sync.asset_uploads
scene_events = DirtyRouterHelper::events_from_scene_changes(&changes)
commands = DirtyCommandBuffer::default()

DirtyRouterHelper::route_events(TEXTURE_STAGE_RULES, &scene_events, scene, &mut commands)
commands.apply_texture_commands(render_texture_manager)
texture_result = RenderTextureManager.update(asset_uploads.pending_texture_uploads)
texture_events = DirtyRouterHelper::events_from_texture_update_result(texture_result)
DirtyRouterHelper::route_events(AFTER_TEXTURE_STAGE_RULES, &texture_events, scene, &mut commands)
commands.apply_sky_commands(render_sky_manager)
commands.apply_material_commands(render_material_manager)

DirtyRouterHelper::route_events(SKY_STAGE_RULES, &scene_events, scene, &mut commands)
commands.apply_sky_commands(render_sky_manager)
sky_result = RenderSkyManager.update(scene, render_texture_manager, asset_uploads.pending_sky_distribution_uploads)

DirtyRouterHelper::route_events(MATERIAL_STAGE_RULES, &scene_events, scene, &mut commands)
commands.apply_material_commands(render_material_manager)
commands.apply_emissive_commands(render_emissive_light_table)
material_result = RenderMaterialManager.update(...)
material_events = DirtyRouterHelper::events_from_material_update_result(material_result)
DirtyRouterHelper::route_events(AFTER_MATERIAL_STAGE_RULES, &material_events, scene, &mut commands)
commands.apply_instance_commands(render_instance_manager)
commands.apply_emissive_commands(render_emissive_light_table)

DirtyRouterHelper::route_events(MESH_STAGE_RULES, &scene_events, scene, &mut commands)
commands.apply_mesh_commands(render_mesh_manager)
commands.apply_instance_commands(render_instance_manager)
commands.apply_emissive_commands(render_emissive_light_table)
mesh_result = RenderMeshManager.update(asset_uploads.pending_mesh_uploads)
mesh_events = DirtyRouterHelper::events_from_mesh_update_result(mesh_result)
DirtyRouterHelper::route_events(AFTER_MESH_STAGE_RULES, &mesh_events, scene, &mut commands)
commands.apply_instance_commands(render_instance_manager)
commands.apply_emissive_commands(render_emissive_light_table)

DirtyRouterHelper::route_events(INSTANCE_STAGE_RULES, &scene_events, scene, &mut commands)
commands.apply_instance_commands(render_instance_manager)
instance_result = RenderInstanceManager.update(...)
instance_events = DirtyRouterHelper::events_from_instance_update_result(instance_result)
DirtyRouterHelper::route_events(AFTER_INSTANCE_STAGE_RULES, &instance_events, scene, &mut commands)
commands.apply_tlas_commands(render_tlas_manager)
commands.apply_emissive_commands(render_emissive_light_table)

DirtyRouterHelper::route_events(ANALYTIC_STAGE_RULES, &scene_events, scene, &mut commands)
commands.apply_analytic_light_commands(render_analytic_light_manager)
analytic_result = RenderAnalyticLightManager.update(...)

emissive_result = RenderEmissiveLightTable.update(...)

RenderTlasManager.update(...)
RenderWorld.update_scene_root(...)
```

这个顺序保证 texture ready 能在同一 prepare 内标记 material dirty 和 sky dirty，material stable slot 新建、
替换或失效也能在同一 prepare 内标记 instance material binding dirty。`RenderWorld` 可以对外实现
`RenderSceneView`，但不应把内部 manager owner 暴露给 render pass。

实现时，`RenderWorld::prepare` 是跨 manager 借用的唯一组织点。它可以在局部作用域内拆借多个
`RenderXXXManager`，但 `DirtyRouterHelper` 不直接接收 `&mut RenderXXXManager`。helper 只把
`DirtyEvent` 转成 `DirtyCommandBuffer`；命令 apply 由 `RenderWorld::prepare` 在阶段边界完成。
如果 Rust 借用要求更细粒度的作用域，应拆成 `RenderWorld` 内部私有方法，而不是让各 manager 互相持有引用。

### `RenderSceneView`

`RenderSceneView` 是 render pass 访问 GPU scene 的最小只读契约，接口定义位于
`truvis-render-foundation::render_scene_view`，由 `RenderWorld` 实现。具体 render pass 不依赖
`RenderWorld` 的 concrete type，也不访问任何 `RenderXXXManager`。

`RenderWorld` 是 Rust 侧 render-side owner / prepared world 名称；shader-visible scene root ABI
可以继续使用现有 `gpu::scene::GpuScene` 或等价生成绑定名。本文的 owner 重命名不要求同步修改
shader struct 名称、descriptor layout、root buffer 字段或 shader include 路径；这些 ABI 名称只表达
shader 侧布局，不代表 Rust 侧 owner 仍叫 `GpuScene`。

目标接口保持窄能力：

```rust
pub trait RenderSceneView {
    fn scene_buffer_device_address(&self, frame_label: FrameLabel) -> vk::DeviceAddress;
    fn tlas_handle(&self, frame_label: FrameLabel) -> Option<vk::AccelerationStructureKHR>;
    fn accum_signature(&self, frame_label: FrameLabel) -> RenderSceneAccumSignature;
    fn draw_raster(
        &self,
        frame_label: FrameLabel,
        cmd: &GfxCommandBuffer,
        before_draw: &mut dyn FnMut(u32, u32),
    );
}

pub struct RenderSceneAccumSignature {
    pub tlas_revision: u64,
    pub emissive_light_version: u32,
    pub analytic_light_version: u32,
    pub sky_distribution_version: u32,
}
```

`RenderSceneView` 不暴露 material buffer、instance buffer、BLAS、TLAS owner、sky owner、light table owner 或
bindless manager 的可变访问。pass 只能读取 scene root device address、TLAS handle、accum signature，
或通过 `draw_raster` 提交当前 frame label 的 raster draw cache。

### Scene root buffer contract

scene root buffer 是 shader-visible scene 的入口表，由 `RenderWorld.update_scene_root(...)` 在 prepare
末尾写入。它只保存 device address、bindless handle、count 和 version，不复制大块 scene 数据。
目标字段应参考当前 shader scene root 合同：

- material buffer address：`all_mats`
- geometry table address：`all_geometries`
- stable instance slot buffer address：`all_instances`
- instance-local material / geometry indirect map：`instance_material_map`、`instance_geometry_map`
- analytic light buffer address 与 count
- emissive triangle records、alias table、instance emissive base map、record count、alias count、enabled flag 和 version
- sky SRV、sky sampler type、sky importance distribution address、width / height、enabled flag、intensity 和 version

`RenderSceneAccumSignature` 从 scene root 同步点派生，只包含会让离线 progressive accumulation 失效的版本：
TLAS revision、emissive light version、analytic light version 和 sky distribution version。它不暴露具体 buffer
布局或资源 owner。
`sky_distribution_version` 可沿用当前字段名，但目标语义是 sky-visible version：enabled、intensity、sky texture
binding 或 distribution buffer 任一 shader-visible sky 状态变化时都应更新。

### Render manager FIF 与回收通用规则

所有 `RenderXXXManager` 的 FIF buffer、free list、retired slot、dirty set、timeline upload 和延迟释放策略
都应参考当前 runtime 实现，不为新命名重新发明资源生命周期规则。
`RenderSkyManager` 和 light owners 虽然不都叫 `RenderXXXManager`，但其 GPU buffer / image / descriptor
资源同样遵守显式 destroy、deferred release 和 frame token 推进规则。

- per-FIF buffer 以 `FrameLabel` 为索引；当前 `frame_label` 对应的 buffer 只能在当前 prepare / upload
  中写入，CPU 不能覆盖 GPU 仍可能读取的上一轮 FIF buffer。
- `begin_frame(frame_token)` 或等价阶段负责推进 frame id，并作为跨 FIF 回收窗口的时间基准。
- 删除后的 stable slot 不立即回到 `free_slots`；至少跨过 `FrameCounter::fif_count()` 个 frame id 后，
  才能复用给新 material / instance，避免 in-flight command buffer 用旧 slot 读到新对象。
- free list、retired slot、dirty bit 和 per-FIF buffer 状态都属于对应 render manager；
  `DirtyRouterHelper` 只生成 dirty command，不保存这些生命周期状态。
- GPU resource owner 必须显式 destroy / deferred release；`Drop` 只允许暴露遗漏释放，不负责释放
  Vulkan / VMA / WSI 资源。shutdown 路径可以等待 upload timeline 后统一销毁未完成资源。
- v1 继续使用固定容量的 material / instance / geometry / light 等 render-side buffer / slot 池；
  容量耗尽属于 render runtime v1 约束失败，沿用当前 fatal / panic / expect 风格，不把 capacity
  管理上移到 `SceneStore`。
- 多 submesh 不表示无限容量。v1 使用固定总容量，而不是按每个 instance 固定 submesh 上限：
  `max_instance_count` 限制 stable instance slot，`max_geometry_count` 限制 shader-visible geometry table，
  `max_instance_submesh_indirect_count` 限制 `instance_geometry_map` / `instance_material_map` 的总 entry 数。
  一个 instance 可以引用任意数量 submesh，但所有 active instances 展开后的 indirect entry 总数不得超过
  `max_instance_submesh_indirect_count`；超出时沿用当前 runtime fatal / panic / expect 风格。

### `RenderTextureManager`

`RenderTextureManager` 是 `RenderWorld` 内部的 render-side texture GPU 缓存 owner，key 使用
`TextureHandle`。

维护状态：

- `TextureHandle -> UploadedSceneTexture`
- pending GPU upload queue
- upload timeline / command pool / staging buffer 生命周期
- fallback texture
- `TextureHandle -> uploaded_revision`
- GPU 状态：`NotUploaded` / `Uploading` / `Ready` / `Failed`
- 本帧 ready 状态发生变化的 texture 集合

目标状态形状：

```rust
pub enum RenderTextureGpuStatus {
    NotUploaded,
    Uploading {
        revision: u64,
        timeline: UploadTimelineValue,
    },
    Ready {
        uploaded_revision: u64,
    },
    Failed {
        revision: u64,
    },
}
```

```rust
pub struct RenderTextureUpdateResult {
    pub ready_changed_textures: Vec<TextureHandle>,
}
```

`ready_changed_textures` 表示 texture resolver 可见状态发生变化：包括 upload 完成进入 ready、ready texture
被替换 / 删除后失效，以及 upload failed 后需要继续使用 fallback 的状态变化。

`UploadedSceneTexture` 保存 GPU 资源身份：

```rust
pub struct UploadedSceneTexture {
    pub image_handle: GfxImageHandle,
    pub view_handle: GfxImageViewHandle,
    pub srv_handle: BindlessSrvHandle,
    pub sampler: gpu::bindless::ESamplerType,
    pub uploaded_revision: u64,
}
```

GPU 上传状态只属于 render-side manager。`SceneStore` 只知道 CPU bytes 是否 ready，不知道 GPU image
是否已经创建或注册 bindless。

`RenderTextureManager` 的异步上传规则参考当前 texture upload queue：

- `TextureCpuData` 只作为短期 upload payload；提交 texture upload 后立即释放 CPU bytes。
- upload queue 使用单调递增 timeline value；完成检测在后续 update 中非阻塞查询，不等待 GPU。
- image 在 upload timeline 完成前保持 manager 私有状态，不注册到 bindless table，也不进入 texture resolver ready。
- timeline 到达后先检查当前 manager 中的 `TextureHandle + revision` 仍匹配，且 GPU 状态仍是对应
  upload；handle stale、revision 不匹配或状态已被替换时，只销毁完成资源，不 publish ready，也不写
  ready changed。检查通过后释放 staging buffer，注册 image / view / SRV，并把对应 texture 加入 ready changed 集合。
- 替换或删除旧 texture 时，先让旧 GPU 可见状态失效并注销 SRV，再按 resource owner / FIF 安全边界释放旧 image / view。

### `RenderMeshManager`

`RenderMeshManager` 是 `RenderWorld` 内部的 render-side mesh GPU 缓存 owner，负责 vertex / index buffer
上传、BLAS 构建和 shader-visible geometry table / geometry slots。mesh 上传完成、BLAS 可用和 mesh GPU
ready 状态都由它记录；`SceneStore` 只保存 mesh 的 CPU 语义 metadata 和 revision。
一个 `MeshHandle` 可以对应多个 submesh；`RenderMeshManager` 必须为同一 scene mesh 维护对应的
多个 `RtGeometry` / geometry records / geometry slots / submesh metadata。一个 submesh 对应一个
shader-visible geometry slot；一个 mesh 的 BLAS 包含多个 geometry input。

维护状态：

- `MeshHandle -> UploadedSceneMesh`
- vertex / index device buffer
- BLAS 和 BLAS device address
- shader-visible geometry records / geometry slots
- derived triangle metadata，例如 `RtTriangleMeta`，用于 emissive triangle table
- pending mesh upload / BLAS build queue
- `MeshHandle -> uploaded_revision`
- GPU 状态：`NotUploaded` / `Uploading` / `Ready` / `Failed`
- 本帧 ready 状态发生变化的 mesh 集合

目标状态形状：

```rust
pub enum RenderMeshGpuStatus {
    NotUploaded,
    Uploading {
        revision: u64,
        timeline: UploadTimelineValue,
    },
    Ready {
        uploaded_revision: u64,
    },
    Failed {
        revision: u64,
    },
}
```

```rust
pub struct RenderMeshUpdateResult {
    pub ready_changed_meshes: Vec<MeshHandle>,
}
```

`RenderMeshManager.update(...)` 消费 `PendingMeshUpload`，提交 vertex / index upload 和 BLAS build。
`MeshCpuData` 在 upload submission 后释放，`RenderMeshManager` 不长期保存完整 mesh CPU 数据。它可以在
upload submission 前从 `MeshCpuData` 派生并保存轻量 triangle metadata，供 `RenderEmissiveLightTable`
计算 triangle area、primitive id 和 submesh-local record。
upload queue 使用单调递增 timeline value；完成检测非阻塞。timeline 完成前，vertex / index buffer、
BLAS、geometry slots 和 derived triangle metadata 不进入 ready resolver，也不能被 active instance / TLAS
消费。timeline 到达后先检查当前 manager 中的 `MeshHandle + revision` 仍匹配，且 GPU 状态仍是对应
upload；handle stale、revision 不匹配或状态已被替换时，只销毁完成 buffer / BLAS / scratch 资源，不
publish ready，也不写 ready changed。检查通过后释放 staging / scratch 等临时资源，发布 uploaded mesh，
并把 mesh 加入 ready changed 集合。替换或删除旧 mesh 时，先让旧 mesh ready 状态失效，再按 owner / FIF
安全边界释放旧 buffer、BLAS 和 geometry slots。
ray tracing shader 中 `GeometryIndex()` 等于 instance-local submesh index，`PrimitiveIndex()` 是
submesh-local primitive index。`RenderInstanceManager` 写入的 `instance_geometry_map` 和
`instance_material_map` 必须按同一个 submesh index 对齐，raycast 反查也按该 index 解析 scene material。
依赖这些 mesh 的 instance 是否可以 active，由 `DirtyRouterHelper` 通过 `SceneStore` 的反向依赖索引路由给
`RenderInstanceManager`，而不是让 mesh manager 直接修改 instance 状态。

面向 emissive table 的只读 view 必须是 lightweight borrowed view。字段只能是 slice、引用、handle、
range 或轻量 resolver / iterator；不得包含 owned `Vec`、克隆的 material 参数、复制的 mesh vertex/index
数据，或每帧为 emissive table 临时收集的大数组。view 必须直接借用 manager 已维护的稳定快照或缓存。
换言之，不变量是：不得包含 owned Vec，不得为 view 形状 clone 或 collect 大块输入数据。

`EmissiveMeshView` 借用 `RenderMeshManager` 已派生保存的 `RtTriangleMeta` / submesh metadata，不复制完整
mesh CPU data：

```rust
pub struct EmissiveMeshView<'a> {
    pub triangle_metadata: &'a [RtTriangleMeta],
    pub submeshes: &'a [SceneSubmesh],
}
```

### `RenderMaterialManager`

`RenderMaterialManager` 是 `RenderWorld` 内部的 shader-visible material buffer owner，负责把
`SceneMaterial` 转换为 shader-visible material buffer。

维护状态：

- `MaterialHandle -> stable material slot` 映射
- `free_slots` stable material slot 池
- dirty slot 列表：`slot -> SlotDirtyInfo { fif_dirty: [bool; FIF], dirty_frame_id }`
- 每个 frame-in-flight 的 material buffer 和 staging buffer
- material revision / uploaded revision
- 延迟 slot 回收状态
- 本帧 stable slot 新建、替换或失效的 material 集合

```rust
pub struct RenderMaterialUpdateResult {
    pub slot_changed_materials: Vec<MaterialHandle>,
}
```

`RenderMaterialManager` 查询 `RenderTextureManager` 的 texture resolver。texture 未上传完成或上传失败时，
resolver 返回 fallback binding，材质仍保持 shader 可读，不阻塞 instance 进入 GPU scene。
`RenderMaterialManager` 不持有完整 material 参数的长期副本；dirty material 上传时从 `SceneStore`
读取当前 `SceneMaterial`，现场解析 texture binding 并写入当前 frame-in-flight 的 staging buffer。
`RenderMaterialUpdateResult` 用于通知 `DirtyRouterHelper` 哪些 material 的 stable slot 可见性发生变化；
material 参数 dirty 仍由 `RenderMaterialManager` 自己持有。

`RenderMaterialManager` 的 slot / FIF 规则参考当前 `RenderMaterialManager`：

- stable material slot 从 `free_slots` 分配，handle 生命周期内 slot 保持稳定。
- 注册 material、更新 material 参数或 texture 从 fallback 切换到真实 binding 时，对应 slot 标记为所有
  FIF dirty：`fif_dirty = [true; FrameCounter::fif_count()]`。
- upload 只处理当前 `frame_label` 对应的 dirty bit；当前 FIF 写完后清掉该 bit，所有 FIF bit 清空后移除 dirty record。
- 删除 material 时移除 handle -> slot 映射，slot 内容置空，不再上传；仅保留 `dirty_frame_id` 用于延迟回收计时。
- 当前 frame id 与 `dirty_frame_id` 的差值达到 `FrameCounter::fif_count()` 后，slot 才能回到 `free_slots`。

面向 emissive table，`RenderMaterialManager` 不提供材质参数 view；材质参数的权威值来自
`SceneStore`。它只提供 `MaterialHandle -> stable material slot` 的只读 resolver，不暴露
material manager 的可变内部状态：

```rust
pub trait RenderMaterialSlotResolver {
    fn material_slot(&self, material: MaterialHandle) -> Option<u32>;
}
```

### `RenderInstanceManager`

`RenderInstanceManager` 是 `RenderWorld` 内部的 render-side instance GPU 表示 owner。它持有 stable instance slot，并负责
instance buffer、`instance_geometry_map` 和 `instance_material_map` 的上传。它不主动扫描或比较
`SceneStore` revision，而是根据 `RenderWorld::prepare` apply 的 dirty command 读取当前 scene / material / mesh 状态。

维护状态：

- `InstanceHandle -> stable instance slot`
- `free_slots` stable instance slot 池
- `retired_slots { slot, retired_frame_id }`
- pending / active 状态
- dirty instance set
- 每个 frame-in-flight 的 instance buffer 和 staging buffer
- 每个 frame-in-flight 的 `instance_geometry_map` / `instance_material_map`
- slot 延迟回收状态
- raycast / hit test 所需的 slot 到 scene handle 反查表
- motion history reset 状态，用于把 previous transform 与 current transform 对齐

dirty 类型至少区分：

```rust
pub enum InstanceDirtyKind {
    Lifecycle,
    Transform,
    MeshBinding,
    MaterialBinding,
}
```

`RenderInstanceManager::mark_dirty(instance, kind)` 只写入 dirty 标记。后续 upload 阶段根据 dirty
从 `SceneStore` 读取 instance 的 mesh、material list 和 transform，从 `RenderMeshManager` 查询 mesh ready /
geometry slot，从 `RenderMaterialManager` 查询 stable material slot。只有 mesh ready 且 material slot 全部可解析的
instance 才进入 active；pending / active 转换由 instance manager 记录，但不直接写 TLAS dirty。
`RenderInstanceManager` 持有的 `instance_geometry_map` 指向 `RenderMeshManager` 管理的 geometry slots；
`instance_material_map` 指向 `RenderMaterialManager` 管理的 stable material slots。两个 map 必须按
instance-local submesh 顺序对齐：同一个 local submesh index 同时索引 geometry slot、material slot、
emissive base map 和 raycast 反查所需的 material 语义。

`RenderInstanceManager` 的 slot / FIF 规则参考当前 `RenderInstanceManager`：

- `free_slots` 初始化为固定容量 stable instance slot 池；新 instance 分配 slot 后先进入 pending。
- instance 只有在 mesh ready 且 material stable slot 全部可解析时才进入 active；texture fallback 不阻塞 active。
- removed / stale instance 移除 scene handle -> stable slot 映射后进入 `retired_slots { slot, retired_frame_id }`。
- `begin_frame(frame_token)` 或等价阶段回收已经跨过 `FrameCounter::fif_count()` 个 frame id 的 retired slot。
- instance buffer、`instance_geometry_map` 和 `instance_material_map` 按当前 `frame_label` 写入，不覆盖其他 FIF buffer。
- manager 不主动全量比对 `SceneStore` revision；它只消费 dirty 标记，并按需读取 `SceneStore`、
  `RenderMeshManager` 和 `RenderMaterialManager` 的当前状态。
- `request_motion_history_reset()` 或等价 API 属于 `RenderInstanceManager` / `RenderWorld` 的 render-side
  状态，不进入 `SceneChanges`。history reset 时，即使 CPU transform 没变，也要把 active instance 的
  previous transform 写成 current transform，避免 DLSS / motion vector 使用旧 slot 历史；这只标记 instance
  buffer 需要更新，不应单独标记 TLAS dirty，除非 transform 本身发生变化。

```rust
pub struct RenderInstanceUpdateResult {
    pub active_set_changed: bool,
    pub transform_changed_instances: Vec<InstanceHandle>,
    pub mesh_binding_changed_instances: Vec<InstanceHandle>,
    pub material_binding_changed_instances: Vec<InstanceHandle>,
}
```

`RenderInstanceUpdateResult` 会先规范化成 `DirtyEvent`，再由 `AFTER_INSTANCE_STAGE_RULES` 生成
TLAS dirty / emissive dirty command。
`RenderInstanceManager` 不写入 TLAS dirty，避免 dirty routing 分散。

面向 emissive table 的只读 view 只暴露 active instance、transform、instance-local submesh 顺序以及
geometry/material indirect 起点；`RenderEmissiveLightTable` 不能修改 instance slot 或 active gate。
如果 `RenderInstanceManager` 内部已有连续 active snapshot，可以使用 slice view：

```rust
pub struct ActiveInstanceView<'a> {
    pub instances: &'a [ActiveInstanceRecord],
}
```

如果 active instances 在内部不是连续存储，目标接口应使用 `ActiveInstanceIter<'a>` 或 resolver view，
不得为了返回 slice 构造临时 `Vec<ActiveInstanceRecord>`，也不得每帧为 emissive table 重建整份 active set。

### `RenderTlasManager`

`RenderTlasManager` 是 `RenderWorld` 内部的 TLAS owner，单独持有每个 frame-in-flight 的 TLAS、dirty 状态和
build / reuse / destroy 逻辑。它不重新判断 instance 的 active gate，避免和 `RenderInstanceManager`
维护两份 active 状态。

维护状态：

- per-FIF TLAS
- TLAS dirty set / revision
- build scratch / instance build input 的短期资源
- 当前 TLAS 覆盖的 scene revision 或 dirty reason

dirty reason 至少覆盖：

```rust
pub enum TlasDirtyReason {
    ActiveInstanceSetChanged,
    InstanceTransformChanged,
    InstanceMeshBindingChanged,
    MeshReadyChanged,
}
```

`RenderTlasManager::mark_dirty(reason)` 只记录需要重建或释放 TLAS。build 阶段通过
`RenderInstanceManager` 获取 active TLAS instance 输入，通过 `RenderMeshManager` 获取 BLAS address；
空 active set 时销毁当前 frame label 的 TLAS，让 render pass 通过 `tlas_handle == None` 识别空场景。
TLAS 是 per-FIF resource：每个 `FrameLabel` 独立持有 TLAS 与 `tlas_revision`。当前 frame label 的 TLAS
只在 dirty 且 active input / BLAS binding / transform 覆盖范围变化时 rebuild；scene root buffer、
instance buffer、geometry table、material / geometry indirect map 和 analytic light buffer 都遵循同一 per-FIF
buffer 模型，只写当前 frame label 的资源。

### `RenderSkyManager`

`RenderSkyManager` 是 `RenderWorld` 内部的 sky / environment GPU 派生资源 owner，负责 sky fallback binding、
sky texture binding、importance sampling distribution buffer、distribution version、dirty 状态和 retired resource
延迟释放。CPU 权威状态来自 `SceneStore.SceneSkyState`；`RenderSkyManager` 不直接请求 `AssetHub`，
不长期持有 `TextureCpuData`，也不修改 `SceneStore`。

维护状态：

- fallback sky texture binding 和 fallback distribution
- 当前 sky texture 的 resolver 结果、enabled / intensity 快照和 uploaded sky revision
- sky distribution device buffer / staging buffer / dimensions
- `sky_distribution_version` 或等价 sky-visible version
- sky dirty 状态和 dirty reason
- deferred retired distribution buffers / fallback resources

目标接口形状：

```rust
pub enum SkyDirtyReason {
    SkyStateChanged,
    SkyTextureReadyChanged,
    SkyDistributionChanged,
}

pub struct RenderSkyUpdateResult {
    pub sky_version_changed: bool,
}
```

`RenderSkyManager.update(...)` 消费 `SceneAssetSyncOutput.pending_sky_distribution_uploads`，并读取
`SceneStore.SceneSkyState` 与 `RenderTextureManager` 的 texture resolver。它只在 dirty 或收到新的
`PendingSkyDistributionUpload` 时更新自己的 GPU binding / distribution buffer。sky texture 未 ready、GPU upload
失败、distribution build 失败或 distribution upload 失败时，scene root 应继续使用 fallback texture /
fallback distribution；这些失败不反向污染 `SceneStore` 的 CPU scene 语义状态。

`PendingSkyDistributionUpload` 的 `scene_texture + texture_revision + sky_revision` 必须在 update 时和当前
`SceneStore` / `RenderTextureManager` 状态匹配。handle stale、texture revision 不匹配、sky 已切换到其他
texture 或 sky revision 不匹配时，`RenderSkyManager` 丢弃该 payload，不 publish 新 distribution version。
`RenderSkyManager` 可以参考当前 `RenderSkyManager` 的 fallback binding、distribution buffer、version 和 retired
resource 策略，但目标 owner 收敛为 `RenderWorld` 内部 manager。

### `RenderAnalyticLightManager`

`RenderAnalyticLightManager` 是 `RenderWorld` 内部的 analytic light buffer owner，负责显式 analytic light
的 structured buffer、staging buffer、count 和 `analytic_light_version`。analytic light CPU 参数继续由
`SceneStore` 持有；v1 保留一个 `LightHandle` 语义即可，不引入 point / spot / area 三套独立 scene
handle，也不做 per-light dirty。它只消费 `SceneStore` 的 analytic light snapshot，不依赖 texture、
mesh、material、instance 或 TLAS manager。

CPU 侧目标形状是统一 handle + kind：

```rust
pub struct SceneAnalyticLight {
    pub kind: SceneAnalyticLightKind,
    pub revision: u64,
}

pub enum SceneAnalyticLightKind {
    Point(ScenePointLight),
    Spot(SceneSpotLight),
    Area(SceneAreaLight),
}
```

`RenderAnalyticLightManager` update 时可以把统一 light snapshot 拆成 point / spot / area 三类 GPU arrays，
以保持现有 shader scene root contract；这只是 render-side packing 细节，不改变 CPU scene handle 体系。

维护状态：

- analytic light device buffer
- analytic light staging buffer
- 当前 light count 和 uploaded revision
- `analytic_light_version`
- dirty 状态

目标接口：

```rust
pub struct RenderAnalyticLightUpdateResult {
    pub analytic_light_version_changed: bool,
}
```

`SceneChanges.changed_analytic_lights` 会规范化成 `DirtyEvent::SceneAnalyticLightsChanged`，
并通过 `ANALYTIC_STAGE_RULES` 生成 analytic light dirty command。update 阶段从 `SceneStore` 读取全部 analytic light 快照并
全量重建 / 上传 analytic light buffer。analytic light 不创建 TLAS 可命中的发光几何，也不需要 material slot、
geometry slot 或 texture binding；它只通过 `RenderWorld` scene root 写入 device address、count 和 version。

### `RenderEmissiveLightTable`

`RenderEmissiveLightTable` 是 `RenderWorld` 内部的 emissive triangle sampling table owner，负责从当前
active instances、mesh derived triangle metadata、`SceneStore` CPU 材质参数 view 和 render-side
material slot resolver 构建自发光三角形 NEE / hit PDF 所需的 GPU buffer。

维护状态：

- `emissive_triangle_lights`
- `emissive_light_alias_table`
- `instance_emissive_triangle_base_map`
- 每个 frame-in-flight 的 staging buffer / device buffer
- emissive table dirty 状态和 revision

它依赖以下 lightweight borrowed view / resolver，而不是直接读写各 manager 的内部状态：

- 从 `RenderInstanceManager` 读取 `ActiveInstanceView`：active instance、transform、instance-local submesh 顺序、
  geometry/material indirect 起点。
- 从 `RenderMeshManager` 读取 `EmissiveMeshView`：submesh metadata、derived triangle metadata、primitive id、
  local area。
- 从 `SceneStore` 读取 `SceneMaterialEmissiveResolver`：按 `MaterialHandle` 查询 base color、
  emissive、opaque 等 CPU 权威参数。
- 从 `RenderMaterialManager` 读取 `RenderMaterialSlotResolver`：当前 GPU 上可见的 stable material slot。

`RenderEmissiveLightTable` 不依赖 `RenderTlasManager`。TLAS 和 emissive table 都消费 active instance / mesh /
material 派生状态，但彼此不是输入关系。v1 emissive power 使用常量 material 参数；如果未来要把 base color /
emissive texture 纳入 power 估计，应通过 texture ready -> material dirty -> emissive dirty 的链路接入。

目标接口：

```rust
pub enum EmissiveDirtyReason {
    MeshReadyChanged,
    MaterialChanged,
    MaterialSlotChanged,
    InstanceActiveSetChanged,
    InstanceTransformChanged,
    InstanceMeshBindingChanged,
    InstanceMaterialBindingChanged,
}

pub struct RenderEmissiveLightUpdateResult {
    pub emissive_light_version_changed: bool,
}
```

mesh ready、material changed / slot changed、instance active set / transform / binding 变化会通过
`DirtyEvent` 和 `DirtyRuleKind` 生成 emissive dirty command。update 阶段读取上述只读 view，重建并上传 emissive
buffers；scene root 只消费最终 buffer address、record count 和 version。`RenderEmissiveLightTable`
可以重建自己的输出 buffer，但输入 view 本身必须是零大块拷贝的只读访问层。

### `DirtyRouterHelper`

`DirtyRouterHelper` 是 `RenderWorld::prepare` 内部使用的 stateless helper，统一处理跨 `SceneStore`、
texture / sky / mesh / material / instance / TLAS / analytic light / emissive table 的 invalidation 推导。
它不是 owner，不是 manager，不作为 `RenderWorld` 字段存在；它不拥有 GPU resource，不保存本帧
`SceneChanges`，不保存 material 或 instance 参数，也不执行 upload、pack、build 或资源释放。

每帧 `World::sync_for_render()` 只调用一次 `SceneStore::drain_changes() -> SceneChanges`，并把结果放入
`WorldRenderSync.scene_changes`。`RenderWorld::prepare` 将 `scene_changes` 作为局部变量保存；
`DirtyRouterHelper` 不直接修改任何 render manager，而是把 scene change / update result 规范化成
`DirtyEvent`，再用静态 `DirtyRuleKind` rule set 生成 `DirtyCommand`。`RenderWorld::prepare`
在阶段边界把命令 apply 到对应 owner。其他 manager 不直接 drain scene change log，也不互相推导 dirty。

目标接口形状：

```rust
pub struct DirtyRouterHelper;

pub enum DirtyEvent {
    SceneTextureRemoved(TextureHandle),
    TextureReadyChanged(TextureHandle),

    SceneSkyChanged,

    SceneMaterialChanged(MaterialHandle),
    SceneMaterialRemoved(MaterialHandle),
    MaterialSlotChanged(MaterialHandle),

    SceneMeshRemoved(MeshHandle),
    MeshReadyChanged(MeshHandle),

    SceneInstanceChanged(InstanceHandle, InstanceDirtyKind),
    SceneInstanceRemoved(InstanceHandle),
    InstanceActiveSetChanged,
    InstanceTransformChanged,
    InstanceMeshBindingChanged,
    InstanceMaterialBindingChanged,

    SceneAnalyticLightsChanged,
}

pub enum DirtyRuleKind {
    SceneTextureRemovedMarksTextureRemoved,
    TextureReadyChangedMarksDependentMaterials,
    TextureReadyChangedMarksSky,
    SceneSkyChangedMarksSky,
    SceneMaterialChangedMarksMaterial,
    SceneMaterialRemovedMarksMaterialRemoved,
    MaterialChangedMarksEmissive,
    MaterialSlotChangedMarksDependentInstances,
    MaterialSlotChangedMarksEmissive,
    SceneMeshRemovedMarksMeshRemoved,
    MeshReadyChangedMarksDependentInstances,
    MeshReadyChangedMarksEmissive,
    SceneInstanceChangedMarksInstance,
    SceneInstanceRemovedMarksInstanceRemoved,
    InstanceUpdateMarksTlas,
    InstanceUpdateMarksEmissive,
    SceneAnalyticLightsChangedMarksAnalytic,
}

pub enum DirtyCommand {
    MarkTextureRemoved(TextureHandle),
    MarkSkyDirty(SkyDirtyReason),
    MarkMaterialDirty(MaterialHandle),
    MarkMaterialRemoved(MaterialHandle),
    MarkMeshRemoved(MeshHandle),
    MarkInstanceDirty(InstanceHandle, InstanceDirtyKind),
    MarkInstanceRemoved(InstanceHandle),
    MarkTlasDirty(TlasDirtyReason),
    MarkAnalyticLightsDirty,
    MarkEmissiveDirty(EmissiveDirtyReason),
}

pub struct DirtyCommandBuffer {
    // 本帧局部命令缓冲；内部按 target manager / table 分组，并按 handle / reason 合并。
    texture: TextureDirtyCommands,
    sky: SkyDirtyCommands,
    material: MaterialDirtyCommands,
    mesh: MeshDirtyCommands,
    instance: InstanceDirtyCommands,
    tlas: TlasDirtyCommands,
    analytic: AnalyticDirtyCommands,
    emissive: EmissiveDirtyCommands,
}

impl DirtyCommandBuffer {
    pub fn apply_texture_commands(&mut self, manager: &mut RenderTextureManager);
    pub fn apply_sky_commands(&mut self, manager: &mut RenderSkyManager);
    pub fn apply_material_commands(&mut self, manager: &mut RenderMaterialManager);
    pub fn apply_mesh_commands(&mut self, manager: &mut RenderMeshManager);
    pub fn apply_instance_commands(&mut self, manager: &mut RenderInstanceManager);
    pub fn apply_tlas_commands(&mut self, manager: &mut RenderTlasManager);
    pub fn apply_analytic_light_commands(&mut self, manager: &mut RenderAnalyticLightManager);
    pub fn apply_emissive_commands(&mut self, table: &mut RenderEmissiveLightTable);
}

impl DirtyRouterHelper {
    pub fn events_from_scene_changes(changes: &SceneChanges) -> Vec<DirtyEvent>;

    pub fn events_from_texture_update_result(result: RenderTextureUpdateResult) -> Vec<DirtyEvent>;

    pub fn events_from_material_update_result(result: RenderMaterialUpdateResult) -> Vec<DirtyEvent>;

    pub fn events_from_mesh_update_result(result: RenderMeshUpdateResult) -> Vec<DirtyEvent>;

    pub fn events_from_instance_update_result(result: RenderInstanceUpdateResult) -> Vec<DirtyEvent>;

    pub fn route_events(
        rules: &[DirtyRuleKind],
        events: &[DirtyEvent],
        scene: SceneReadView<'_>,
        out: &mut DirtyCommandBuffer,
    );
}
```

`DirtyRuleKind` 使用 enum + 静态 slice，不使用字符串配置、运行时 rule 注册、trait object registry
或完整 ECS scheduler。每条 rule 只允许通过 `SceneReadView` 读取 `SceneStore` 的反向依赖和
sky / material / mesh / instance
关系，并向 `DirtyCommandBuffer` 写入命令；它不接收 `&mut RenderXXXManager`。

推荐静态 rule set：

```rust
pub const TEXTURE_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::SceneTextureRemovedMarksTextureRemoved,
];

pub const AFTER_TEXTURE_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::TextureReadyChangedMarksDependentMaterials,
    DirtyRuleKind::TextureReadyChangedMarksSky,
];

pub const SKY_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::SceneSkyChangedMarksSky,
];

pub const MATERIAL_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::SceneMaterialChangedMarksMaterial,
    DirtyRuleKind::SceneMaterialRemovedMarksMaterialRemoved,
    DirtyRuleKind::MaterialChangedMarksEmissive,
];

pub const AFTER_MATERIAL_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::MaterialSlotChangedMarksDependentInstances,
    DirtyRuleKind::MaterialSlotChangedMarksEmissive,
];

pub const MESH_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::SceneMeshRemovedMarksMeshRemoved,
];

pub const AFTER_MESH_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::MeshReadyChangedMarksDependentInstances,
    DirtyRuleKind::MeshReadyChangedMarksEmissive,
];

pub const INSTANCE_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::SceneInstanceChangedMarksInstance,
    DirtyRuleKind::SceneInstanceRemovedMarksInstanceRemoved,
];

pub const AFTER_INSTANCE_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::InstanceUpdateMarksTlas,
    DirtyRuleKind::InstanceUpdateMarksEmissive,
];

pub const ANALYTIC_STAGE_RULES: &[DirtyRuleKind] = &[
    DirtyRuleKind::SceneAnalyticLightsChangedMarksAnalytic,
];
```

这些 rule set 只声明 dirty 传播关系；prepare 顺序仍由 `RenderWorld::prepare` 手写控制，因为 FIF、
timeline、slot 回收、TLAS build 和 manager update result 都依赖明确阶段边界。

语义等价的分段路由规则：

```text
SceneStore texture removed
  -> DirtyCommand::MarkTextureRemoved(texture)

RenderTextureUpdateResult.ready_changed_textures
  -> SceneStore.materials_using_texture(texture)
  -> DirtyCommand::MarkMaterialDirty(material)
  -> if SceneStore.sky_uses_texture(texture): DirtyCommand::MarkSkyDirty(SkyTextureReadyChanged)

SceneStore sky environment changed
  -> DirtyCommand::MarkSkyDirty(SkyStateChanged)

SceneStore material changed
  -> DirtyCommand::MarkMaterialDirty(material)
  -> DirtyCommand::MarkEmissiveDirty(MaterialChanged)

SceneStore material removed
  -> DirtyCommand::MarkMaterialRemoved(material)
  -> DirtyCommand::MarkEmissiveDirty(MaterialChanged)

RenderMaterialUpdateResult.slot_changed_materials
  -> SceneStore.instances_using_material(material)
  -> DirtyCommand::MarkInstanceDirty(instance, MaterialBinding)
  -> DirtyCommand::MarkEmissiveDirty(MaterialSlotChanged)

SceneStore mesh removed
  -> DirtyCommand::MarkMeshRemoved(mesh)
  -> DirtyCommand::MarkEmissiveDirty(MeshReadyChanged)

RenderMeshUpdateResult.ready_changed_meshes
  -> SceneStore.instances_using_mesh(mesh)
  -> DirtyCommand::MarkInstanceDirty(instance, MeshBinding)
  -> DirtyCommand::MarkEmissiveDirty(MeshReadyChanged)

SceneStore instance changed
  -> DirtyCommand::MarkInstanceDirty(instance, kind)

SceneStore instance removed
  -> DirtyCommand::MarkInstanceRemoved(instance)

RenderInstanceUpdateResult
  -> DirtyCommand::MarkTlasDirty(ActiveInstanceSetChanged / InstanceTransformChanged / InstanceMeshBindingChanged)
  -> DirtyCommand::MarkEmissiveDirty(InstanceActiveSetChanged / InstanceTransformChanged / InstanceMeshBindingChanged / InstanceMaterialBindingChanged)

SceneStore analytic lights changed
  -> DirtyCommand::MarkAnalyticLightsDirty
```

`DirtyCommandBuffer` 是 `RenderWorld::prepare` 的本帧局部对象，不跨帧保存。它负责合并命令：

- 同一 resource 的 removed command 强于 dirty command；removed 不应降级成普通 dirty。
- 同一个 CPU scene instance dirty kind 按强度保留最强项：`Lifecycle > MaterialBinding > Transform`。
- TLAS / emissive / sky reason 使用集合或 bitflags 合并，避免重复 mark。
- 命令按目标 owner 分组，例如 texture、sky、material、mesh、instance、TLAS、analytic、emissive。
- `RenderWorld::prepare` 在阶段边界调用对应 `apply_*_commands(...)`，只把该分组写入对应
  manager / table 的 dirty 或 removed 集合。
- apply 后应清空已消费的命令集合，避免后续阶段重复写入已经处理过的 manager。

`DirtyCommandBuffer` 只是 command coalescing buffer。它不拥有资源、不查询 render manager、不执行
upload / build / pack / free；真正的资源处理仍由 `RenderWorld::prepare` 调用各 `RenderXXXManager.update(...)`
或 light / emissive owner update 完成。

`World::sync_for_render()` 是唯一的 `SceneStore` change log drain 入口；`RenderWorld::prepare`
只消费 `WorldRenderSync.scene_changes`。`DirtyRouterHelper` 是唯一的 invalidation routing helper。
其他 manager 只接收 `RenderWorld::prepare` 从 `DirtyCommandBuffer` apply 的 dirty 标记，或返回 update result
供 helper 规范化为下一批 `DirtyEvent`，避免 dirty 推导散落到多个 manager 内部。
removed change 不应降级成普通 `mark_dirty`：对应 render manager 必须提供明确的
`mark_removed` / `remove_*` 路径，用于失效 resolver、注销 bindless / GPU-ready 状态、移除 handle -> slot
映射，并按各自 FIF / retired 规则延迟释放资源或 stable slot。由于 `SceneStore` 在 removed 后已经让
handle stale，render manager 处理 removed 时不能依赖 `SceneStore::get_*` 读取被删资源。

## 删除与资源回收规则

CPU scene 删除只发生在 `SceneStore`。删除 CPU resource handle 后，SlotMap 立即使旧 handle 失效；
`SceneStore` 写入 removed change 并清理相关反向依赖索引，但不释放任何 GPU 资源。

删除 texture / material / mesh 前必须先检查 `SceneStore` 维护的反向依赖索引：

- 删除 texture 时，`TextureHandle -> Vec<MaterialHandle>` 必须为空，且 sky 不得引用该 texture。
- 删除 material 时，`MaterialHandle -> Vec<InstanceHandle>` 必须为空。
- 删除 mesh 时，`MeshHandle -> Vec<InstanceHandle>` 必须为空。
- 删除 instance 不需要依赖检查，直接允许。

如果存在依赖，`SceneStore` 返回 `SceneDeleteError::StillReferenced { dependents }`，并且不删除资源、
不推进 revision、不写入 `SceneChanges`。只有依赖检查通过后，才使 SlotMap handle stale、写入
removed change 并清理反向依赖索引。

render-side 回收由对应 owner 独立完成：

- `RenderTextureManager` 处理 removed texture：从自己的 `TextureHandle -> UploadedSceneTexture`
  表中移除记录，注销 bindless SRV，再按 frame-in-flight 安全边界释放 image / view。
- `RenderMeshManager` 处理 removed mesh：让 mesh GPU ready 失效，释放或延迟释放 vertex / index buffer、
  BLAS、geometry slots 和 derived triangle metadata；依赖该 mesh 的 instance 由 `DirtyRuleKind`
  通过反向依赖生成 instance dirty command，再由 `RenderWorld::prepare` apply 到 `RenderInstanceManager`。
- `RenderMaterialManager` 处理 removed material：移除 handle -> stable slot 映射，slot 内容置空，
  并延迟至少一个 FIF 窗口后回收到 free list，避免 in-flight shader 读到被新材质复用的 slot。
- `RenderInstanceManager` 处理 removed instance：移除 scene handle -> stable instance slot 映射，
  active instance set 发生变化时返回 `RenderInstanceUpdateResult`，stable slot 延迟至少一个 FIF 窗口后回收。
- `RenderTlasManager` 不直接解释 CPU 删除；它只消费 `RenderWorld::prepare` 从 instance update result
  派生命令中 apply 过来的 TLAS dirty reason，并在 dirty 时基于当前 active instance set rebuild / destroy TLAS。

删除后的 in-flight asset completion 不得复活资源。`SceneAssetIngestor` 收到迟到的 loader event 时，如果等待映射
已经不存在，或映射到的 CPU resource handle 已经 stale，必须 drop event payload 并结束处理。

## Texture 注册过程

当创建一个 instance 且已知 texture path 时，App / importer 应通过 `World` facade 先注册 texture，
再注册 material 和 instance；`SceneStore` 的 `register_*` 只作为内部实现入口。

```text
App / importer
  -> World.register_texture(path, import_desc) -> TextureHandle
  -> World.register_material({ diffuse_texture: Some(scene_texture), ... }) -> MaterialHandle
  -> World.register_instance({ mesh, materials, transform }) -> InstanceHandle
```

`World::register_texture` 对文件 path 在 facade 层完成 filesystem canonicalize 后，构造
`TextureSource::File { canonical_path }`，再调用 `SceneStore` 内部 texture 注册逻辑，根据
`SceneTextureKey = TextureSource + TextureImportDesc` 分配或复用 `TextureHandle`。同一 key 永远返回
同一个 handle，不会阻塞读取文件，也不会创建 GPU image。
注册前必须对 filesystem path 执行 filesystem canonicalize；canonicalize 成功后的 path 进入
`TextureSource::File { canonical_path }`，失败时返回错误，不创建 `TextureHandle`，也不写入 change log。
成功注册 texture 也不写入 `SceneChanges`；后续 CPU load ready 通过 `PendingTextureUpload` 进入
`RenderTextureManager`，GPU ready 状态变化再通过 `RenderTextureUpdateResult.ready_changed_textures`
驱动 material / sky dirty。

`SceneTexture` 初始状态：

```text
source = TextureSource::File { canonical_path }
import = TextureImportDesc { color_space, generate_mips, ... }
cpu_status = PendingLoad
revision = new revision
```

## Model / FBX 导入过程

App 不直接调用 `AssetHub` 加载 FBX，也不每帧查询 `AssetHub` 的 model 状态。App 只向
`World` 创建“把该 model 导入当前 scene”的请求：

```text
App / UI
  -> World::request_model_import(path, import_desc) -> ModelImportHandle
  -> World forwards request to internal SceneAssetIngestor
  -> SceneAssetIngestor submits AssetHub.request_model(ModelLoadDesc)
  -> AssetHub emits ModelLoadEvent
  -> SceneAssetIngestor builds ModelImportPlan
  -> SceneStore::import_model_transaction(plan)
```

`AssetHub` 的 model event 只携带 loader handle、desc 和 owned CPU 导入数据，不携带
CPU resource handle、`Asset*Handle` 或 GPU resource handle：

```rust
pub enum ModelLoadEvent {
    Ready {
        handle: ModelLoadHandle,
        desc: ModelLoadDesc,
        data: ModelCpuData,
    },
    Failed {
        handle: ModelLoadHandle,
        desc: ModelLoadDesc,
        error: String,
    },
}

pub struct ModelCpuData {
    pub source: ModelSource,
    pub name: String,
    pub meshes: Vec<ModelMeshImport>,
    pub materials: Vec<ModelMaterialImport>,
    pub instances: Vec<ModelInstanceImport>,
}
```

`ModelCpuData` 中的依赖全部使用 event-local index。loader 不分配 scene handle，也不保存对
`SceneStore` 的引用：

```rust
pub struct ModelMeshImport {
    pub name: String,
    pub submeshes: Vec<ModelSubmeshImport>,
    pub local_aabb: Aabb,
}

pub struct ModelSubmeshImport {
    pub name: String,
    pub positions: Vec<glam::Vec3>,
    pub normals: Vec<glam::Vec3>,
    pub tangents: Vec<glam::Vec3>,
    pub uvs: Vec<glam::Vec2>,
    pub indices: Vec<u32>,
    pub local_aabb: Aabb,
}

pub struct ModelMaterialImport {
    pub name: String,
    pub base_color: glam::Vec4,
    pub emissive: glam::Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub opaque: f32,
    pub diffuse_texture: Option<TextureImportRef>,
    pub normal_texture: Option<TextureImportRef>,
}

pub struct TextureImportRef {
    pub source: TextureSource,
    pub import: TextureImportDesc,
}

pub struct ModelInstanceImport {
    pub name: String,
    pub mesh: ModelMeshIndex,
    pub materials: Vec<ModelMaterialIndex>,
    pub transform: glam::Mat4,
}
```

`ModelInstanceImport.mesh` 引用 `ModelCpuData.meshes` 中的局部 mesh index；
`ModelInstanceImport.materials` 引用 `ModelCpuData.materials` 中的局部 material index。
`ModelInstanceImport.materials.len()` 必须等于对应 `ModelMeshImport.submeshes.len()`；第 `i` 个
material index 对应第 `i` 个 submesh / geometry。所有 index 都必须在 `SceneAssetIngestor` ingest
阶段验证；任何越界、缺失或 submesh/material 对应关系不完整，都应让整个 model import 失败，不能留下半注册 scene。
实现上应先校验 event-local 依赖并构造待注册计划，确认能完整注册后再写入 `SceneStore`。
一个 `ModelMeshImport` 可以包含多个 `ModelSubmeshImport`；每个 `ModelSubmeshImport` 都是完整 geometry。
导入后同一个 `MeshHandle` 保留这些 submesh metadata，后续 `RenderMeshManager` 和
`RenderInstanceManager` 必须继续以 instance-local
submesh 顺序维护 geometry / material indirect map。

FBX / glTF importer v1 contract 参考当前 Assimp / truvixx 导入路径：

- loader task 只复制 owned CPU 数据；C++ scene handle、raw pointer 或 FFI 临时 slice 不跨出 loader 边界。
- 每个 `ModelSubmeshImport` 都要求 positions、normals、tangents、uvs、indices 均可用；v1 不在 scene ingest
  阶段补建缺失 normal / tangent / uv，也不修复拓扑。
- material import 复制 base color、emissive、metallic、roughness、opacity，以及 diffuse / normal texture path。
- texture path 先保留 importer 返回的表达，再由 asset ingest 根据 canonical model source 解析成 filesystem
  path 并执行 filesystem canonicalize；canonicalize 失败时整个 model import failed，不降级为 lexical
  normalize。diffuse / normal 用途只决定 `SceneMaterial` 槽位，不进入 `SceneTextureKey`。
- instance import 使用 importer 提供的 world transform。一个 source node 引用多个 mesh 时，可以拆成多条
  `ModelInstanceImport`，每条记录使用 event-local mesh index 和 material index list。
- importer 或 ingest 任一步失败都让整个 model import failed；不自动重试，不留下半注册 scene。

`SceneAssetIngestor` 成功 ingest model event 时先验证 event-local index，并构造 `ModelImportPlan`。
`ModelImportPlan` 只包含 texture / mesh metadata / material / instance 的 CPU 语义注册数据，不拥有 GPU
resource，也不把 vertex / index bytes 写入 `SceneStore`。`SceneStore::import_model_transaction(plan)`
按固定顺序原子注册：

```text
1. register textures from all ModelMaterialImport texture refs
2. register mesh metadata and build mesh_index -> MeshHandle map
3. register materials and build material_index -> MaterialHandle map
4. register instances using the two maps
```

transaction 失败时不写入 scene、不推进 revision、不污染反向依赖索引。成功时 texture 注册只创建或复用
`TextureHandle`，并把 texture 置为 pending load；mesh 注册只保存长期 metadata，不长期保存
vertex / index CPU bytes。transaction 返回 `ModelImportResult`，其中 mesh result 至少包含
`MeshHandle + revision`，供 `SceneAssetIngestor` 生成
`PendingMeshUpload { scene_mesh, revision, data }`，提交给 render-side 后释放。mesh、material、instance
注册会推进对应 scene resource revision；其中 mesh 添加通过 `PendingMeshUpload` 同步到 render side，
material / instance 添加通过 `SceneChanges.changed_materials` / `SceneChanges.changed_instances` 同步。

model import v1 只对 texture 按 `SceneTextureKey` 去重；mesh、material 和 instance 不做跨 import 去重。
即使两次导入同一个 model path，同一个 `MeshSource::Model { model, mesh_index }` 也会创建新的
`MeshHandle`，material 同理创建新的 `MaterialHandle`。`MeshSource` 只作为 provenance /
debug / UI 查询信息，不作为 mesh dedupe key。若未来需要 mesh/material dedupe，应单独引入
`SceneMeshKey` / `SceneMaterialKey` 和引用计数 / 删除策略。

目标形状：

```rust
pub struct ModelImportPlan {
    pub textures: Vec<TextureImportPlan>,
    pub meshes: Vec<MeshMetadataImportPlan>,
    pub materials: Vec<SceneMaterialImportPlan>,
    pub instances: Vec<SceneInstanceImportPlan>,
}

pub struct ModelImportResult {
    pub textures: Vec<TextureHandle>,
    pub meshes: Vec<ImportedMeshResult>,
    pub materials: Vec<MaterialHandle>,
    pub instances: Vec<InstanceHandle>,
}

pub struct ImportedMeshResult {
    pub scene_mesh: MeshHandle,
    pub revision: u64,
}
```

## 阶段流程

### Update 阶段：创建和编辑 CPU scene

App / UI 在此阶段只提交导入意图或编辑已有 CPU scene；FBX / glTF 等格式 importer 不直接写
`SceneStore`，model 内容只在 asset sync 阶段由 `SceneAssetIngestor` ingest：

- 调用 `World::request_model_import` 创建 model import 请求，不直接注册 model 内容
- 新增 texture path
- 新增或修改 material
- 新增或修改 instance
- 修改 texture path / import desc 时注册或复用新的 `TextureHandle`，再更新 material / sky 引用；
  不修改已有 handle 的 `SceneTextureKey`

这些操作只改变 CPU 语义状态或 loader 协调状态，并推进对应 scene resource revision。此阶段不提交
GPU upload。

### Asset Sync 阶段：提交和回收 CPU 加载任务

`RenderRuntime` 在 App update 之后、`RenderWorld.prepare(...)` 之前调用
`World::sync_for_render() -> WorldRenderSync`。`World::sync_for_render()` 是 CPU scene change log 与短期
render upload payload 的边界，begin-frame 只推进 FIF 回收和 frame token。

`World::sync_for_render()` 内部先让 `SceneAssetIngestor` 提交尚未发送给 `AssetHub` 的 model import 请求：

```text
ModelImportHandle
  -> ModelLoadDesc
  -> AssetHub.request_model(desc) -> ModelLoadHandle
  -> record ModelImportHandle <-> ModelLoadHandle
```

随后 drain `AssetHub` 的 model 完成事件：

```text
ModelLoadEvent::Ready(load_handle, desc, data)
  -> lookup ModelImportHandle
  -> validate event-local mesh/material/submesh indices
  -> build ModelImportPlan
  -> SceneStore::import_model_transaction(plan)
  -> enqueue PendingMeshUpload for imported meshes
  -> mark import Ready

ModelLoadEvent::Failed(load_handle, desc, error)
  -> lookup ModelImportHandle
  -> mark import Failed(error)
```

model ingest 完成后，FBX / glTF 中引用到的 texture 会注册为 `SceneStore` 中的 scene texture metadata；
`SceneAssetIngestor` 同步把需要 CPU decode 的 texture 放入自己的 pending texture load queue。这避免
每帧全量扫描 `SceneStore`，也让 texture 添加不需要进入 `SceneChanges`。

`SceneAssetIngestor` 消费内部 pending texture load queue：

```text
TextureHandle + TextureLoadDesc + SceneTextureLoadPurpose
  -> TextureLoadDesc
  -> AssetHub.request_texture(desc) -> TextureLoadHandle
  -> record TextureLoadHandle -> SubmittedTextureLoad { scene_texture, texture_revision, purpose }
  -> SceneStore.mark_texture_loading(scene_texture)
```

随后 `SceneAssetIngestor` drain `AssetHub` 的 texture 完成事件：

```text
TextureLoadEvent::Ready(load_handle, desc, data)
  -> lookup SubmittedTextureLoad { scene_texture, texture_revision, purpose }
  -> SceneStore.finish_texture_load(scene_texture)
  -> scene texture revision += 1
  -> if purpose includes sky distribution:
       derive PendingSkyDistributionUpload(scene_texture, texture_revision, sky_revision, &data)
  -> if purpose includes texture upload:
       enqueue PendingTextureUpload(scene_texture, revision, data)
     else:
       drop TextureCpuData after distribution payload is derived

TextureLoadEvent::Failed(load_handle, desc, error)
  -> lookup SubmittedTextureLoad
  -> SceneStore.fail_texture_load(scene_texture, error)
  -> scene texture revision += 1
```

`AssetHub` 在事件生成后不再持有 loader handle、desc 或 texture bytes。`SceneStore` 也不长期保存
`TextureCpuData`；CPU bytes 只保存在 `SceneAssetIngestor` 的短期上传收件箱中，直到 render-side
提交对应 upload。

`World::sync_for_render()` 返回并清空本帧短期 upload payload，同时 drain `SceneStore` 的 CPU 语义变化：

```rust
pub struct WorldRenderSync {
    pub scene_changes: SceneChanges,
    pub asset_uploads: SceneAssetSyncOutput,
}

pub struct SceneAssetSyncOutput {
    pub pending_texture_uploads: Vec<PendingTextureUpload>,
    pub pending_sky_distribution_uploads: Vec<PendingSkyDistributionUpload>,
    pub pending_mesh_uploads: Vec<PendingMeshUpload>,
}

pub struct PendingTextureUpload {
    pub scene_texture: TextureHandle,
    pub revision: u64,
    pub data: TextureCpuData,
}

pub struct PendingSkyDistributionUpload {
    pub scene_texture: TextureHandle,
    pub texture_revision: u64,
    pub sky_revision: u64,
    pub data: SkyDistributionCpuData,
}

pub struct SkyDistributionCpuData {
    pub width: u32,
    pub height: u32,
    pub entries: Vec<SkyDistributionEntryCpu>,
}

pub struct PendingMeshUpload {
    pub scene_mesh: MeshHandle,
    pub revision: u64,
    pub data: MeshCpuData,
}
```

sky / environment distribution build 使用短期 `TextureCpuData` 派生出的 `SkyDistributionCpuData`，不要求
`SceneStore`、`SceneAssetIngestor` 或 `RenderSkyManager` 长期保存 HDRI bytes。`PendingSkyDistributionUpload`
提交给 `RenderSkyManager` 后释放；如果提交时 `scene_texture + texture_revision + sky_revision` 已经 stale，
则丢弃该 payload。
`SkyDistributionEntryCpu` 的字段应对应当前 shader 使用的 alias probability、solid angle pdf 和 alias index
语义；它是 distribution upload 的输入，不是原始 texture 像素副本。

### Prepare 阶段：RenderWorld 分段同步

`RenderRuntime::prepare` 在 `World::sync_for_render()` 之后调用
`RenderWorld.prepare(scene: SceneReadView<'_>, sync: WorldRenderSync, render_view)`。`RenderWorld`
消费 `WorldRenderSync.asset_uploads` 中的短期 upload payload 和 `WorldRenderSync.scene_changes`，
并通过只读 view 读取 `SceneStore` 的当前语义状态。prepare 阶段不直接调用 `SceneStore::drain_changes()`；
`DirtyRouterHelper` 静态函数在各 manager update 之间分段把 dirty event 转成 command。

固定顺序：

```text
RenderRuntime::prepare:
  sync = World::sync_for_render()
  RenderWorld.prepare(scene, sync):
    changes = sync.scene_changes
    asset_uploads = sync.asset_uploads
    scene_events = DirtyRouterHelper::events_from_scene_changes(&changes)
    commands = DirtyCommandBuffer::default()

    DirtyRouterHelper::route_events(TEXTURE_STAGE_RULES, &scene_events, scene, &mut commands)
    commands.apply_texture_commands(render_texture_manager)
    texture_result = RenderTextureManager.update(asset_uploads.pending_texture_uploads)
    texture_events = DirtyRouterHelper::events_from_texture_update_result(texture_result)
    DirtyRouterHelper::route_events(AFTER_TEXTURE_STAGE_RULES, &texture_events, scene, &mut commands)
    commands.apply_sky_commands(render_sky_manager)
    commands.apply_material_commands(render_material_manager)

    DirtyRouterHelper::route_events(SKY_STAGE_RULES, &scene_events, scene, &mut commands)
    commands.apply_sky_commands(render_sky_manager)
    sky_result = RenderSkyManager.update(scene, render_texture_manager, asset_uploads.pending_sky_distribution_uploads)

    DirtyRouterHelper::route_events(MATERIAL_STAGE_RULES, &scene_events, scene, &mut commands)
    commands.apply_material_commands(render_material_manager)
    commands.apply_emissive_commands(render_emissive_light_table)
    material_result = RenderMaterialManager.update()
    material_events = DirtyRouterHelper::events_from_material_update_result(material_result)
    DirtyRouterHelper::route_events(AFTER_MATERIAL_STAGE_RULES, &material_events, scene, &mut commands)
    commands.apply_instance_commands(render_instance_manager)
    commands.apply_emissive_commands(render_emissive_light_table)

    DirtyRouterHelper::route_events(MESH_STAGE_RULES, &scene_events, scene, &mut commands)
    commands.apply_mesh_commands(render_mesh_manager)
    commands.apply_instance_commands(render_instance_manager)
    commands.apply_emissive_commands(render_emissive_light_table)
    mesh_result = RenderMeshManager.update(asset_uploads.pending_mesh_uploads)
    mesh_events = DirtyRouterHelper::events_from_mesh_update_result(mesh_result)
    DirtyRouterHelper::route_events(AFTER_MESH_STAGE_RULES, &mesh_events, scene, &mut commands)
    commands.apply_instance_commands(render_instance_manager)
    commands.apply_emissive_commands(render_emissive_light_table)

    DirtyRouterHelper::route_events(INSTANCE_STAGE_RULES, &scene_events, scene, &mut commands)
    commands.apply_instance_commands(render_instance_manager)
    instance_result = RenderInstanceManager.update()
    instance_events = DirtyRouterHelper::events_from_instance_update_result(instance_result)
    DirtyRouterHelper::route_events(AFTER_INSTANCE_STAGE_RULES, &instance_events, scene, &mut commands)
    commands.apply_tlas_commands(render_tlas_manager)
    commands.apply_emissive_commands(render_emissive_light_table)

    DirtyRouterHelper::route_events(ANALYTIC_STAGE_RULES, &scene_events, scene, &mut commands)
    commands.apply_analytic_light_commands(render_analytic_light_manager)
    analytic_result = RenderAnalyticLightManager.update()

    emissive_result = RenderEmissiveLightTable.update()

    RenderTlasManager.update()
    RenderWorld.update_scene_root()
```

#### Texture update

`RenderTextureManager.update(...)` 消费 `PendingTextureUpload`。提交 upload 后立即释放
`TextureCpuData`：

```text
for each PendingTextureUpload { scene_texture, revision, data }:
  if current status is not Ready { uploaded_revision: revision }:
      submit texture GPU upload(data)
      gpu_status = Uploading { revision, timeline }
  drop TextureCpuData after upload submission

timeline reached:
  -> if handle stale, revision mismatch, or status is not matching Uploading { revision, timeline }:
       destroy completed image / staging resources
       do not publish ready
       do not append ready_changed_textures
  -> release staging / command buffer
  -> register image into GfxResourceManager
  -> create image view
  -> register SRV into ShaderBindingSystem / bindless table
  -> textures[TextureHandle] = UploadedSceneTexture
  -> gpu_status = Ready
  -> RenderTextureUpdateResult.ready_changed_textures += scene_texture
```

替换同一个 `TextureHandle` 的旧 GPU texture 时，旧 SRV 先从 bindless 表注销，再通过 render-side
资源管理器按 frame-in-flight 安全边界释放。`RenderTextureUpdateResult.ready_changed_textures`
会被规范化成 `DirtyEvent::TextureReadyChanged`；`AFTER_TEXTURE_STAGE_RULES` 根据
`SceneStore.materials_using_texture(texture)` 和 sky texture 依赖生成 material dirty / sky dirty command。

#### Sky update

`RenderSkyManager.update(...)` 处理 sky dirty 和 `PendingSkyDistributionUpload`。它读取
`SceneStore.SceneSkyState` 作为 enabled、intensity、sky texture handle 和 sky revision 的权威值，并通过
`RenderTextureManager` 的 resolver 获取当前 sky texture binding：

```text
for each PendingSkyDistributionUpload { scene_texture, texture_revision, sky_revision, data }:
  if current SceneSkyState.texture != scene_texture
     or current sky revision != sky_revision
     or RenderTextureManager texture revision != texture_revision:
       drop SkyDistributionCpuData
       do not publish sky version
  else:
       upload sky distribution buffer(data)
       replace current distribution after upload submission / completion boundary
       sky_distribution_version += 1
       drop SkyDistributionCpuData

if sky state dirty:
  read enabled / intensity / texture from SceneStore
  resolve texture through RenderTextureManager
  if texture is not GPU ready or sky distribution is unavailable:
      use fallback texture / fallback distribution
  update current sky binding and scene-root-visible sky version when shader-visible state changes
```

标量 intensity 或 enabled 改变不需要重建 alias table，但仍会改变 shader-visible sky binding / scene root，
因此需要更新 sky-visible version，保证 progressive accumulation 能失效重算。

#### Material update

`RenderMaterialManager.update(...)` 处理 dirty material。它只记录 stable slot、dirty 状态和
uploaded revision；材质参数的权威值始终来自 `SceneStore`：

```text
for each dirty SceneMaterial:
  read current SceneMaterial from SceneStore
  resolve diffuse_texture through RenderTextureManager
  resolve normal_texture through RenderTextureManager
  if texture is not GPU ready:
      use fallback binding
  ensure / update stable material slot
  write PbrMaterial into current frame label staging buffer
  copy dirty regions into material device buffer
  if stable slot was created, replaced, or invalidated:
      RenderMaterialUpdateResult.slot_changed_materials += material
```

`RenderMaterialUpdateResult.slot_changed_materials` 会在同一 prepare 内规范化成
`DirtyEvent::MaterialSlotChanged`；`AFTER_MATERIAL_STAGE_RULES` 查询
`SceneStore.instances_using_material(material)`，并生成 instance material binding dirty command。

#### Mesh update

`RenderMeshManager.update(...)` 消费 `PendingMeshUpload`。它提交 vertex / index upload 和 BLAS build；
完成前 mesh 不进入 resolver 可见状态。`MeshCpuData` 在 upload submission 后释放，mesh ready 状态变化只记录在
`RenderMeshManager`，不写回 `SceneStore`。

```text
for each PendingMeshUpload { scene_mesh, revision, data }:
  if current status is not Ready { uploaded_revision: revision }:
      submit vertex/index upload(data)
      submit BLAS build
      gpu_status = Uploading { revision, timeline }
  drop MeshCpuData after upload submission

timeline reached:
  -> if handle stale, revision mismatch, or status is not matching Uploading { revision, timeline }:
       destroy completed buffer / BLAS / scratch resources
       do not publish ready
       do not append ready_changed_meshes
  -> publish UploadedSceneMesh
  -> gpu_status = Ready
  -> RenderMeshUpdateResult.ready_changed_meshes += scene_mesh
```

`RenderMeshUpdateResult.ready_changed_meshes` 会规范化成 `DirtyEvent::MeshReadyChanged`；
`AFTER_MESH_STAGE_RULES` 查询 `SceneStore.instances_using_mesh(mesh)`，并生成 instance mesh binding
dirty command。

#### Instance update

`RenderInstanceManager.update(...)` 根据 dirty instance set 上传 instance GPU 表示。它不主动扫描全量
revision；每个 dirty instance 都从当前 owner 查询所需信息：

```text
for each dirty SceneInstance:
  read mesh / materials / transform from SceneStore
  resolve mesh through RenderMeshManager
  resolve material slots through RenderMaterialManager
  if mesh or any material slot is not ready:
      keep instance Pending
      clear or skip active GPU entry for this slot
  else:
      ensure stable instance slot
      write instance_buffer[slot]
      write instance_geometry_map range
      write instance_material_map range
      mark instance Active
      record RenderInstanceUpdateResult
```

`RenderInstanceManager` 是 stable instance slot、pending / active 状态、instance buffer 和 indirect maps
的 owner。pending / active 转换只发生在这里；它返回 `RenderInstanceUpdateResult`，不写入 TLAS dirty。
该 result 会规范化成 instance update dirty events；`AFTER_INSTANCE_STAGE_RULES` 根据 active set、transform、
mesh binding 和 material binding 变化生成 TLAS dirty / emissive dirty command。

#### Analytic light update

`RenderAnalyticLightManager.update(...)` 只在 analytic light dirty 时读取 `SceneStore` 的 analytic light
快照并全量上传对应 structured buffer：

```text
if RenderAnalyticLightManager is dirty:
  read analytic lights from SceneStore
  upload analytic_light_buffer
  update analytic_light_version
```

analytic light buffer 不依赖 texture、mesh、material、instance 或 TLAS manager。它的 device address、count
和 version 只在 scene root 中汇总给 shader。

#### Emissive light table update

`RenderEmissiveLightTable.update(...)` 只在 emissive dirty 时读取 render-side 只读 view，重建并上传
自发光三角形采样表：

```text
if RenderEmissiveLightTable is dirty:
  active_instances = RenderInstanceManager.emissive_active_instance_view()  // borrowed view
  mesh_view = RenderMeshManager.emissive_mesh_view()                        // borrowed view
  material_params = SceneStore.material_emissive_resolver()               // borrowed resolver
  material_slots = RenderMaterialManager.material_slot_resolver()           // borrowed resolver
  rebuild instance_emissive_triangle_base_map
  rebuild emissive_triangle_lights
  rebuild emissive_light_alias_table
  upload emissive buffers
  update emissive_light_version
```

`RenderEmissiveLightTable` 依赖 `SceneStore`、`RenderMeshManager`、`RenderMaterialManager` 和
`RenderInstanceManager` 提供的 lightweight borrowed view / resolver，但不直接读写这些 manager 的内部状态，
也不要求输入 view 做大块复制。
它不依赖 `RenderTlasManager`；TLAS 和 emissive table
都是 active instance / mesh / material 派生结果，二者互不作为输入。

#### TLAS update

`RenderTlasManager` 只在 `RenderWorld::prepare` apply TLAS dirty command 后工作：

```text
if RenderTlasManager is dirty:
  active_inputs = RenderInstanceManager.active_tlas_instances()
  if active_inputs is empty:
      destroy current frame label TLAS
  else:
      resolve BLAS address through RenderMeshManager
      build or reuse current frame label TLAS
```

TLAS custom index 应使用 stable instance slot，使 ray tracing hit、raster draw、instance buffer 和 raycast
反查共享同一套 instance identity。TLAS manager 不读取 CPU material 参数，也不负责 material / instance upload。

### Raycast / hit test：返回 scene 语义

GPU raycast readback 只返回稳定 instance slot、instance-local submesh index、primitive index、position、
normal、uv 和 hit distance，不写入 CPU handle，也不依赖 `SceneStore` 类型。`RenderInstanceManager`
在 prepare 阶段维护 slot 到 scene handle 的只读反查快照，并在 raycast 解析阶段把 GPU raw hit 转成
App 可理解的 scene 语义：

`SceneRayCastHit` 目标上位于 `truvis-render-runtime::ray_cast`，因为同步 raycast 服务、GPU readback
和反查都属于 runtime；它可以在返回类型中使用 `truvis-world` 的 CPU resource handle。

```rust
pub struct SceneRayCastHit {
    pub position_ws: glam::Vec3,
    pub normal_ws: glam::Vec3,
    pub uv: glam::Vec2,
    pub hit_t: f32,
    pub instance: InstanceHandle,
    pub mesh: MeshHandle,
    pub material: MaterialHandle,
    pub submesh_index: u32,
    pub primitive_index: u32,
}
```

反查快照至少需要记录 `InstanceHandle`、`MeshHandle` 和按 instance-local submesh 顺序排列的
`Vec<MaterialHandle>`。raycast hit 根据 `submesh_index` 查询 material；越界表示 render-side
快照或 shader readback 不一致，应作为 raycast 解析错误处理，而不是返回不完整 hit。

### Render 阶段：只读消费

render pass 不访问 `SceneStore`、`AssetHub` 或 upload queue。它只通过 `RenderSceneView`、global descriptor、
bindless table 和 TLAS handle 读取 prepare 后的 GPU 快照。

## 状态归属表

| 状态 | Owner | 说明 |
| --- | --- | --- |
| App-facing CPU semantic world / asset API | `World` | `World::request_model_import`、`World::register_*`、`World::update_*`、`World::remove_*`、`World::sync_for_render` |
| App-facing edit errors | `World` / `SceneStore` | `WorldEditError` 包装 facade 层错误；`SceneEditError` 表达 scene edit 事务失败 |
| texture source / import desc | `SceneStore` | scene 语义，可查询和编辑；v1 不设计 persistence / stable saved id |
| scene handle live / stale | `SceneStore` | SlotMap handle；删除后旧 handle 查询返回 `None`，不复活 |
| scene texture identity | `SceneStore` | `SceneTextureKey -> TextureHandle`，同一 key 唯一 handle |
| texture CPU load status | `SceneStore` | `PendingLoad` / `Loading` / `Ready` / `Failed` |
| mesh metadata / CPU import status | `SceneStore` | source/import、submesh metadata、AABB、revision；不保存完整 vertex/index bytes |
| loader task status | `AssetHub` | `LoadHandle -> LoadRecord`、后台 IO / decode 状态和完成事件队列；不维护 desc -> handle 去重表 |
| `ModelLoadHandle -> ModelImportHandle` | `SceneAssetIngestor` | model loader 与 scene import 请求的桥接映射 |
| pending / submitted texture load work | `SceneAssetIngestor` | `SceneTextureLoadPurpose` 只在 ingestor 内部决定 upload / sky distribution 产物 |
| `TextureLoadHandle -> SubmittedTextureLoad` | `SceneAssetIngestor` | loader 与 scene texture、revision、load purpose 的桥接映射 |
| decoded CPU bytes | `AssetHub` event -> `SceneAssetIngestor` pending upload -> `RenderTextureManager` upload submission | 提交 upload 后释放，不进入长期 scene 状态 |
| sky distribution build payload | `SceneAssetIngestor -> RenderSkyManager upload submission` | 由短期 `TextureCpuData` 派生 `SkyDistributionCpuData`，提交后释放，不长期保存 HDRI bytes |
| decoded mesh CPU bytes | `AssetHub` event -> `SceneAssetIngestor` pending upload -> `RenderMeshManager` upload submission | 提交 upload 后释放，不进入长期 scene 状态 |
| model CPU import data | `AssetHub` event -> `SceneAssetIngestor` -> `SceneStore::import_model_transaction` | event-local index 解析成 scene handles 后释放 |
| render sync package | `WorldRenderSync` | `World::sync_for_render()` 返回，包含 `SceneChanges` 与 `SceneAssetSyncOutput` |
| pending upload payload | `SceneAssetSyncOutput` | 作为 `WorldRenderSync.asset_uploads` 由 `RenderWorld.prepare(...)` 消费 |
| CPU scene change log | `SceneStore` | 只表达 CPU 语义变化，不表达 GPU upload dirty |
| CPU scene change coalescing | `SceneStore` | 同一 handle 合并变化，removed 强于 changed，instance 保留最强 dirty kind |
| sky / environment CPU state | `SceneStore` | `SceneSkyState` 保存 enabled、intensity、`TextureHandle` 和 revision；`changed_sky_environment` 经 `DirtyEvent` / `DirtyRuleKind` 生成 sky dirty command |
| per-FIF render buffers | 各 `RenderXXXManager` / `RenderWorld` | 按 `FrameLabel` 写入当前 FIF buffer，不覆盖其他 in-flight buffer |
| upload timeline / completion polling | `RenderTextureManager` / `RenderMeshManager` | 单调 timeline value，非阻塞完成检测，完成前不进入 ready resolver |
| GPU image / view / SRV | `RenderTextureManager` | render-side 资源 owner |
| texture GPU ready status | `RenderTextureManager` | 不进入 `SceneStore` |
| mesh GPU ready / BLAS / geometry slots | `RenderMeshManager` | vertex/index buffer、BLAS、shader-visible geometry table 和 ready changed owner |
| texture / material / mesh -> dependent resource 反向依赖 | `SceneStore` | CPU scene 语义索引，用于 dirty 推导；texture 依赖包含 material 和 sky |
| dirty routing | `DirtyRouterHelper` + `DirtyCommandBuffer` | stateless helper 将 changes / update result 转成分组 dirty command；`RenderWorld::prepare` 在阶段边界 apply 到对应 owner |
| material editable params | `SceneStore` | UI 点选和编辑的权威状态 |
| material GPU slot / dirty upload | `RenderMaterialManager` | shader-visible material buffer owner |
| material `free_slots` / `fif_dirty` / `dirty_frame_id` | `RenderMaterialManager` | stable slot 分配、per-FIF dirty upload 和删除后延迟回收 |
| instance GPU slot / active state / instance buffer | `RenderInstanceManager` | stable slot、pending/active、instance buffer 和 indirect maps owner |
| instance `free_slots` / `retired_slots` | `RenderInstanceManager` | stable slot 池和跨 FIF 延迟回收 |
| stable slot delayed reclaim | `RenderMaterialManager` / `RenderInstanceManager` | 删除后至少延迟一个 FIF 窗口再复用 slot |
| TLAS | `RenderTlasManager` | per-FIF TLAS build / reuse / destroy owner |
| analytic light CPU params | `SceneStore` | `LightHandle` 语义和 analytic light snapshot；v1 全量 dirty |
| analytic light GPU buffer | `RenderAnalyticLightManager` | analytic structured buffer、count 和 version owner |
| sky GPU binding / distribution buffer | `RenderSkyManager` | fallback binding、sky texture binding、alias table / distribution buffer、version 和 retired resource owner |
| emissive material CPU resolver | `SceneStore` | `SceneMaterialEmissiveResolver`，按 handle 查询 base color / emissive / opaque 等权威参数 |
| material slot resolver | `RenderMaterialManager` | `MaterialHandle -> stable material slot` 只读 resolver，不提供 material 参数 view |
| emissive triangle derived metadata | `RenderMeshManager` | 从 `MeshCpuData` 派生的轻量 `RtTriangleMeta`，不是完整 mesh CPU data |
| emissive triangle sampling table | `RenderEmissiveLightTable` | emissive records、alias table、instance emissive base map owner |
| scene root buffer / draw cache / TLAS / light views | `RenderWorld` | render pass 只通过 `RenderSceneView` 消费 |
| `RenderSceneView` contract | `truvis-render-foundation` | scene root address、TLAS handle、accum signature、raster draw 的只读 trait |
| shader-visible scene root ABI | shader / generated GPU binding | Rust owner 叫 `RenderWorld`，shader layout 可继续使用 `gpu::scene::GpuScene` |
| fallback texture | `RenderTextureManager` | 保证未 ready texture 仍有合法 binding |

## 失败与 fallback 规则

- CPU 加载失败写入 `SceneStore.SceneTexture.cpu_status = Failed(error)`。
- GPU texture 上传失败写入 `RenderTextureManager` 的 GPU 状态，并继续返回 fallback binding。
- GPU mesh 上传或 BLAS build 失败写入 `RenderMeshManager` 的 GPU 状态；依赖该 mesh 的 instance 保持
  pending / inactive，不写入 instance buffer，不进入 TLAS，也不进入 raster draw。除非用户重新导入或修改
  mesh 引用，否则同一 failed mesh 不自动重试。
- model import 失败写入 `SceneModelImportStatus::Failed(error)`，不写入 scene，不自动重试。
- 失败状态是终止状态；同一个 import / load handle 不重试。用户修改 path / import desc 或重新发起 import
  时，创建新的 request / desc / handle。
- material 引用 failed 或 uploading texture 时不应阻塞整个 instance；shader-visible material 使用 fallback。
- sky texture GPU upload 失败、sky distribution build 失败或 distribution upload 失败时，`RenderSkyManager`
  使用 fallback texture / fallback distribution；这不使 scene import 失败，也不把失败写回 `SceneStore`。
- UI 显示应同时区分 CPU 状态和 GPU 状态：CPU ready 不代表 GPU ready，GPU fallback 不代表材质没有贴图。

## 后续实现收敛点

- 长期 scene 引用使用 `TextureHandle` / `MeshHandle` / `MaterialHandle`；`AssetHub`
  内部 handle 只保留在 `SceneAssetIngestor` 的 loader event 翻译边界。
- 新增 `World` facade，让 App 通过 `World::request_model_import`、`World::model_import_status`、
  `World::register_*`、`World::update_*`、`World::remove_*` 和 `World::sync_for_render`
  使用 scene / asset 能力。
- 新增 `WorldEditError` / `SceneEditError` 边界；失败 edit 保持事务语义，不推进 revision、不写
  `SceneChanges`、不污染反向依赖索引。
- 新增 `SceneAssetIngestor`，作为 `World` 内部对象集中维护 loader handle 到 scene handle 的映射。
- `SceneAssetIngestor` 维护 pending / submitted texture load work，并用 `SceneTextureLoadPurpose`
  区分 `TextureUpload`、`SkyDistributionOnly` 和 `TextureUploadAndSkyDistribution`；`AssetHub`
  不知道这些 scene / render 目的。
- 将 App 直连 `AssetHub` 做 model 加载 / 状态轮询的路径收敛为
  `World::request_model_import` 和 `World::model_import_status`。
- 将 FBX / glTF loader 输出统一为 `ModelCpuData` event 边界，并在 `SceneAssetIngestor` 中完成
  `ModelImportPlan` 构造，再通过 `SceneStore::import_model_transaction` 原子提交。
- 将 texture identity 收敛为 `SceneTextureKey = TextureSource + TextureImportDesc`，同一 key 永远只有一个
  `TextureHandle`；sampler 不进入 texture key。
- 将 source identity 收敛为 `TextureSource` / `MeshSource` / `ModelSource`，并放入 `truvis-asset`
  的公共 source / import 模块；filesystem canonicalize 只适用于 `File` source，procedural / runtime
  source 不走路径规范化。
- `AssetHub` loader 输入收敛为一次性 `TextureLoadDesc` / `ModelLoadDesc`；不维护
  `LoadDesc -> LoadHandle` 去重表，只维护 `LoadHandle -> LoadRecord`。
- 将 mesh / submesh 模型收敛为“一个 submesh 对应一个完整 geometry”，instance 只引用一个 mesh，
  material list 按 submesh index 对齐。
- model import v1 只对 texture 去重；mesh / material / instance 每次 import 都创建新 scene handle。
- 将 sky texture 注册为普通 `TextureHandle`；`SceneStore` 通过 `SceneSkyState` 持有 enabled、
  intensity、texture handle 和 revision，sky / environment owner 不直接请求 `AssetHub` texture。
- 新增 `RenderSkyManager`，把当前 `RenderSkyManager` / environment binding 的 fallback binding、distribution buffer、
  version 和 retired resource 逻辑收敛到 `RenderWorld` 内部 manager，并接入 `DirtyRouterHelper` dirty routing。
- 明确所有 CPU resource handle 为 SlotMap handle，删除后旧 handle 查询不到值；迟到的 loader event 不复活
  stale scene handle。
- 收敛 `SceneStore` edit API：所有语义修改通过 `register_*` / `update_*` / `remove_*`，同步维护
  revision、change log 和反向依赖索引。
- 将 `SceneChanges` 收敛为合并后的 CPU 语义 change log，同一 handle 保留 removed 或最强 dirty kind。
- removed change 通过 `mark_removed` / `remove_*` 进入 render manager 的失效与延迟释放路径，不降级成普通 dirty。
- `RenderTextureManager` 的 key 使用 scene texture handle。
- `RenderMaterialManager` 消费 `MaterialHandle` 和 `TextureHandle`，不直接依赖 asset handle。
- 新增 `DirtyRouterHelper` 静态 helper 和本帧局部 `DirtyCommandBuffer`，集中把 CPU change log 与各
  `RenderXXXManager.update()` / light owner result 转成 dirty event、按静态 rule set 生成 command，
  再由 `RenderWorld::prepare` 分阶段 apply 到 material / instance / TLAS / emissive table 等 owner。
- `DirtyCommandBuffer` 按 target owner 分组并提供 `apply_*_commands(...)`；它只合并 command，不查询
  render manager，不执行 upload / build / pack / free。
- `RenderXXXManager` 的 FIF buffer、free list、retired slot、dirty set、timeline upload 和延迟释放策略
  统一参考当前 runtime 中 `RenderMaterialManager`、`RenderInstanceManager`、`RenderTextureManager` / `RenderMeshManager` 和 `RenderWorld`
  的既有实现模式。
- HDRI alias table、emissive light table、analytic light buffer、scene root buffer 和 `RenderSceneView`
  ABI 可参考当前实现与 `docs/summaries/` 的当前事实；本设计文档不要求本次同步更新 summaries。
- Rust 侧 owner 统一叫 `RenderWorld`；shader-visible scene root ABI 可以继续使用
  `gpu::scene::GpuScene` 或等价生成绑定名，不把 shader layout 重命名纳入本次架构收敛。
- v1 render-side 容量使用固定总容量：`max_instance_count`、`max_geometry_count`、
  `max_instance_submesh_indirect_count`，容量耗尽沿用当前 fatal / panic / expect 风格。
- 将 mesh GPU 上传、BLAS 缓存和 geometry slots 收敛到 `RenderMeshManager`，但不让它长期保存完整
  `MeshCpuData`。
- texture / mesh upload completion 必须检查当前 `CPU resource handle + revision`，stale 或 revision mismatch
  或不匹配当前 `Uploading { revision, timeline }` 的完成资源只销毁，不 publish ready。
- 将 stable instance slot、instance buffer 和 geometry/material indirect maps 收敛到
  `RenderInstanceManager`。
- 保留当前 `RenderInstanceManager` 的 motion history reset 语义：history reset 属于 render-side 状态，
  更新 previous transform / instance buffer，不进入 `SceneChanges`，也不单独触发 TLAS rebuild。
- 将 per-FIF TLAS 从通用 `RenderWorld` 上传流程中拆到 `RenderTlasManager`，并只消费 active instance
  输入与 mesh BLAS address。
- `RenderWorld` 扩展为 render-side prepared world 和 GPU cache owner 聚合体，内部持有全部 `RenderXXXManager`、
  `RenderSkyManager`、`RenderAnalyticLightManager` 与 `RenderEmissiveLightTable`；dirty routing 通过 `DirtyRouterHelper`
  静态函数完成，render pass 仍只通过 `RenderSceneView` 消费快照。
- 在 `RenderWorld` 内明确 scene root buffer contract，并保持 `RenderSceneView` 作为 foundation 层只读 trait，
  不向 render pass 暴露内部 manager owner。
- analytic light buffer 收敛为 `RenderAnalyticLightManager`，只消费 `SceneStore` 的 `LightHandle`
  snapshot，不依赖 texture / mesh / material / instance / TLAS manager；v1 使用全量 dirty / upload。
- emissive light table 收敛为 `RenderEmissiveLightTable`，消费 `RenderMeshManager` /
  `RenderInstanceManager` 的只读 view、`SceneStore` 的 `SceneMaterialEmissiveResolver` 以及
  `RenderMaterialManager` 的 material slot resolver，并接入 `DirtyRouterHelper` dirty routing。
- raycast 命中结果应返回完整 `SceneRayCastHit`，包含 position / normal / uv / hit_t、`InstanceHandle`、
  `MeshHandle`、`MaterialHandle`、submesh index 和 primitive index；UI 再按需查询
  `SceneStore` 的 scene stores。
