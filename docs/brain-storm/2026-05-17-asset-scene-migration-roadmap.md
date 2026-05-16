# Mesh / Material / Instance / Scene 资产化迁移路线（2026-05-17）

本文记录 Mesh、Material、Instance 与 Assimp Scene 加载的长期迁移路线。
它承接 [`2026-05-17-asset-module-responsibility.md`](2026-05-17-asset-module-responsibility.md)
中已经确定的边界：`AssetHub` 只负责内容资产身份与文件到 CPU 内存，
GPU 上传、bindless 注册、BLAS 构建和 shader 可见绑定由 render-side uploader/manager 负责。

## 结论

路线总体可行，但需要先稳定三个边界，再接入 `SceneHandle`：

1. `AssetHub` 的 `Ready` 只表示 CPU 数据已经加载，不表示 GPU 可用。
2. Mesh、Material、Instance 的 GPU slot 只要求在同一个运行时生命周期内稳定。
   slot 可以在注册 / spawn 时分配，不要求不同异步完成顺序下得到相同 slot。
3. `AssetSceneHandle` 表示导入结果或 prefab，不表示已经 spawn 到运行时场景里的 live instance。

因此推荐顺序是：

```text
状态机与 handle 边界
  -> Material 稳定 GPU slot
  -> Mesh AssetHub 管理 + AssetMeshUploader 异步上传 / BLAS
  -> Instance 稳定 slot + ready gate
  -> TLAS dirty / rebuild
  -> Assimp SceneHandle 集成
  -> 清理旧同步 loader 与旧 RenderData 路径
```

## 设计目标

- `AssetHub` 统一管理内容资产身份：Texture、Mesh、Material、Scene。
- GPU 资源创建和 shader 可见绑定全部留在 render-side。
- Mesh 的 vertex/index buffer 上传和 BLAS 构建走异步队列，不阻塞模型加载路径。
- Material 和 Instance 在 GPU scene 中拥有生命周期内稳定 slot。
- Instance 进入 GPU 可见状态前，必须确认引用的 Mesh 和 Material 已经达到 GPU 可用状态。
- Assimp 的文件读取与 CPU 数据抽取进入 `AssetHub`，旧的同步 `AssimpSceneLoader` 已退场。
- 每一步都能独立编译、运行现有示例，并保留可回滚的兼容路径。

## 非目标

- 不要求异步加载完成顺序不同也得到完全相同的 slot。
- 不引入通用 ECS。
- 不让 `AssetHub` 持有 Vulkan/GPU 对象。
- 不让 `AssetSceneHandle` 直接拥有运行时 `InstanceHandle` 生命周期。
- 第一阶段不做 TLAS 增量 refit；先用 dirty 后整棵 rebuild。
- 第一阶段不做资产热重载和跨场景引用计数卸载；只保留未来扩展点。

## 核心术语

| 术语 | 含义 | 所属层 |
|------|------|--------|
| `AssetTextureHandle` | 内容纹理身份，路径去重后得到 | `truvis-asset` |
| `AssetMeshHandle` | 内容 mesh 身份，CPU mesh 数据加载后得到 | `truvis-asset` |
| `AssetMaterialHandle` | 内容 material 身份，保存材质参数和 texture handle | `truvis-asset` |
| `AssetSceneHandle` | 导入后的场景资产 / prefab 身份 | `truvis-asset` |
| `InstanceHandle` | 运行时场景对象身份，来自 spawn/register | `truvis-scene` |
| `GpuMaterialSlot` | material 在 GPU material buffer 中的位置 | render-side |
| `GpuMeshRecord` | mesh 对应的 vertex/index buffer、geometry range、BLAS | render-side |
| `GpuInstanceSlot` | instance 在 GPU instance buffer 中的位置 | render-side |

## 状态机

资产状态必须显式区分 CPU 和 GPU 阶段：

```text
Unloaded
  -> CpuLoading
  -> CpuReady
  -> UploadSubmitted
  -> GpuReady
  -> Failed
```

不同资产可以细化：

```text
Texture:
  CpuReady -> ImageUploadSubmitted -> GpuReady

Mesh:
  CpuReady
    -> BufferUploadSubmitted
    -> BufferReady
    -> BlasBuildSubmitted
    -> GpuReady

Material:
  CpuReady
    -> SlotAllocated
    -> MaterialBufferDirty
    -> GpuReady

Instance:
  Registered
    -> WaitingAssets
    -> Active
    -> RemovedPendingReclaim
```

`AssetHub::get_status()` 只能回答 CPU 侧状态。GPU 可用状态由对应 uploader/manager 回答：

```text
AssetTextureUploader::is_texture_ready(handle)
AssetMeshUploader::is_mesh_ready(handle)
AssetMaterialUploader::is_material_ready(handle)
GpuScene::is_instance_active(handle)
```

## 生命周期内 slot 稳定

“确定位置”的约束定义为：

- Material 注册后分配一个 GPU material slot。
- Instance spawn 后分配一个 GPU instance slot。
- 在该 Material / Instance 生命周期结束前，slot 不变化。
- unregister/despawn 后，slot 不立即复用；等待至少 `FrameCounter::fif_count()` 帧后归还 free list。

这与 `BindlessManager` 和现有 `MaterialManager` 的 dirty / delayed reclaim 模式一致。

不要求：

- 同一个文件每次启动都分配同一个 slot。
- 两个异步任务完成顺序不同也得到相同 slot。
- 删除后重新创建得到原 slot。

## 边界原则

### AssetHub

`AssetHub` 只做：

- 路径去重和 asset handle 分配。
- 后台 IO / Assimp 读取 / CPU 解码。
- 保存 asset record、CPU 状态和 scene asset 内部引用关系。
- 产出 `LoadedAssetEvent`，通知 render-side uploader 有 CPU 数据可消费。

`AssetHub` 不做：

- 创建 `GfxImage` / `GfxBuffer` / `GfxAcceleration`。
- 注册 bindless SRV/UAV。
- 决定 fallback texture。
- 分配 GPU scene slot。
- 持有运行时 live instance 的生命周期。

### Render-Side Uploader / Manager

render-side 负责：

- 消费 `AssetHub` 产出的 CPU 数据。
- 创建 GPU buffer/image/BLAS。
- 管理 timeline semaphore / pending upload / command buffer。
- 维护 `AssetHandle -> GPU record` 的 `SecondaryMap`。
- 对外提供 ready 查询和 resolve 接口。

### SceneManager

`SceneManager` 负责运行时语义：

- spawn / despawn instance。
- 保存 instance transform。
- 保存 instance 引用的 `AssetMeshHandle` 和 `AssetMaterialHandle`。

`SceneManager` 不直接接触：

- bindless slot。
- BLAS device address。
- GPU buffer device address。

### GpuScene

`GpuScene` 负责将运行时 scene 翻译成 shader 可见数据：

- instance buffer。
- instance -> material slot 间接表。
- instance -> geometry slot 间接表。
- TLAS。
- `gpu::GPUScene` 根 buffer。

`GpuScene` 与 `RenderData` 更像 renderer 集成层；Phase 6c 已将它们从
`truvis-render-interface` 上移到 `truvis-render-backend` 私有模块。

## Material 设计

### 目标

Material 在 GPU material buffer 中拥有稳定 slot，并使用 dirty 机制增量上传。

### 推荐结构

```text
AssetHub
  AssetMaterialHandle -> MaterialAssetRecord
    params: base_color / emissive / metallic / roughness / opaque
    textures: Option<AssetTextureHandle>

AssetMaterialUploader
  AssetMaterialHandle -> GpuMaterialSlot
  slots[slot] -> MaterialGpuState
  dirty_slots -> FIF dirty mask + dirty frame id
  pending_texture_ready -> handles
```

backend 私有 `MaterialManager` 已经包含 slot、dirty、FIF 延迟回收和
texture resolver 方向，由 `MaterialBridge` 驱动并作为当前 render-side owner。

### Texture readiness 策略

Instance 不应该直接检查 material 内部 texture 是否 ready。
Instance 只依赖 `AssetMaterialHandle` 的 GPU ready 状态。

Material ready 的定义需要显式选择一种策略：

| 策略 | 行为 | 建议 |
|------|------|------|
| Fallback | texture 未 ready 时写入 fallback/null binding，texture ready 后 material dirty 重传 | 第一阶段默认 |
| Strict | 所有 texture ready 后 material 才算 GpuReady | 可作为导入参数或 debug 模式 |

第一阶段建议使用 Fallback 策略，因为当前 `AssetTextureUploader` 已经持有 fallback
和 `TextureResolver`，能避免大模型因纹理慢加载而整批 instance 不显示。

## Mesh 设计

### 目标

Mesh 由 `AssetHub` 管理内容身份和 CPU 数据，GPU buffer 上传与 BLAS 构建由
`AssetMeshUploader` 管理。

### 推荐结构

```text
AssetHub
  AssetMeshHandle -> MeshAssetRecord
    status: CpuLoading / CpuReady / Failed
    cpu_data: LoadedMeshData

AssetMeshUploader
  AssetMeshHandle -> UploadedMesh
    status: BufferUploadSubmitted / BufferReady / BlasBuildSubmitted / GpuReady
    geometries: Vec<RtGeometry>
    geometry_range: Range<u32>
    blas: GfxAcceleration
    blas_device_address: vk::DeviceAddress
```

### BLAS 异步构建

不要简单地把 `build_blas_sync` 放进后台 CPU 线程。BLAS 构建是 GPU 工作，必须由
渲染线程持有 Vulkan 对象并提交到合适 queue。

第一版建议：

- 使用 graphics queue 提交 BLAS build，避免 transfer queue 不支持 acceleration structure build。
- 复用 `AssetTextureUploader` 的 pending queue 模式：
  - command pool
  - timeline semaphore
  - pending upload/build records
  - `update()` 轮询完成值
- `GpuReady` 只在 timeline 达到对应 value 后置位。
- pending record 持有 staging buffer、scratch buffer、command buffer 和未完成的 BLAS 对象。

### Geometry buffer 归属

当前 `GpuScene` 拥有全局 `geometry_buffer`，并每帧从 `RenderData.all_meshes`
重写。Mesh 资产化后有两种迁移方式：

1. 过渡方案：`GpuScene` 继续拥有 `geometry_buffer`，但写入数据来自 `AssetMeshUploader` 的 ready mesh。
2. 目标方案：`AssetMeshUploader` 拥有 global geometry table，`GpuScene` 只引用其 device address。

推荐先做过渡方案，稳定后再迁移到目标方案。

## Instance 设计

### 目标

Instance 在运行时生命周期内拥有稳定 GPU slot，并在 mesh/material GPU ready 前不进入 active 渲染集。

### 推荐结构

```text
SceneManager
  InstanceHandle -> RuntimeInstance
    mesh: AssetMeshHandle
    materials: Vec<AssetMaterialHandle>
    transform: Mat4

GpuScene
  InstanceHandle -> GpuInstanceSlot
  slots[slot] -> InstanceGpuState
  pending_instances: HashSet<InstanceHandle>
  active_instances: Vec<InstanceHandle>
  dirty_instances: HashMap<slot, SlotDirtyInfo>
```

Instance 注册时即可分配 slot，但该 slot 不一定马上 active。

```text
register instance
  -> allocate stable slot
  -> if dependencies ready: upload slot, mark active, mark TLAS dirty
  -> else: pending_instances

mesh/material becomes ready
  -> recheck pending instances
  -> upload newly active slot
  -> mark TLAS dirty

despawn instance
  -> mark slot removed
  -> mark TLAS dirty
  -> reclaim slot after FIF_COUNT frames
```

### Draw / shader index

当前 `GpuScene::draw()` 用 `RenderData.all_instances` 的连续 index 作为 instance index。
稳定 slot 迁移后，draw path 必须传入 `GpuInstanceSlot`，不能继续依赖临时 Vec index。

初期可以按 slot 顺序维护 active draw list：

```text
active_instances sorted by slot
  -> draw instance
  -> before_draw(instance_slot, submesh_idx)
```

## TLAS 设计

当前 TLAS 构建只在每个 FIF buffer 第一次遇到 scene data 时执行，之后不会自动
响应 mesh ready、instance spawn/despawn 或 transform 变化。

资产化后必须新增 TLAS dirty/revision：

```text
tlas_revision: u64
per_frame_tlas_revision: [u64; FIF_COUNT]

mark_tlas_dirty when:
  - instance becomes active
  - instance despawn
  - instance transform changes
  - instance mesh changes
  - mesh BLAS becomes ready
```

第一阶段不做 refit。只要当前 frame label 的 `per_frame_tlas_revision != tlas_revision`，
就重建当前 FIF 的 TLAS，并销毁旧 TLAS。由于 begin frame 已等待 FIF timeline，
当前 FIF buffer 的旧 TLAS 可以在该 frame label 被重新使用时释放。

TLAS instance 的 `instance_custom_index` 建议使用 `GpuInstanceSlot`。需要保留现有
24-bit 限制检查，超过限制时显式报错，而不是静默截断。

## Assimp / SceneHandle 设计

### 目标

Assimp 从 render-backend 同步 loader 中移出，变成 `AssetHub::load_scene()` 的后台 CPU 加载任务。

### 推荐语义

```text
AssetHub::load_scene(path) -> AssetSceneHandle

AssetSceneHandle:
  表示一个导入后的 scene asset / prefab
  可以查询内部 mesh/material/texture handles
  不表示已经 spawn 到 SceneManager 的 live instances
```

### 数据流

```text
App / tool
  -> AssetHub::load_scene(path)
  -> AssetLoader background task
       truvixx_scene_load(path)
       copy all mesh/material/instance data into owned Rust CPU data
       truvixx_scene_free(scene)
       send LoadedSceneData
  -> AssetHub::update()
       allocate AssetMeshHandle / AssetMaterialHandle / AssetTextureHandle
       fill AssetSceneRecord internal references
       emit LoadedAssetEvent::SceneLoaded
  -> World / SceneSpawner reads AssetSceneRecord
       create runtime InstanceHandle(s) through SceneManager
       each instance references AssetMeshHandle + AssetMaterialHandle(s)
  -> render-side uploaders consume mesh/material/texture CPU events
  -> GpuScene activates instances when dependencies are GpuReady
```

### 重要边界

- 后台 Assimp task 必须复制 owned CPU 数据，不能把 `TruvixxSceneHandle` 或 raw pointer
  跨线程 / 跨帧暴露给 Rust runtime。
- `truvixx_scene_free` 必须在 CPU 数据复制完成后调用。
- `AssetSceneHandle` 查询的是 asset 内部 handle 列表，不直接返回 live `InstanceHandle`。
- 同一个 scene asset 可以被多次 spawn，每次 spawn 产生独立运行时 instance 生命周期。

## 迁移阶段

### Phase 0：状态机和 handle 边界

目标：

- 新增或规划 `AssetMaterialHandle`、`AssetSceneHandle`。
- 明确 `LoadStatus` 只表示 CPU 侧状态。
- 新增 render-side ready resolver trait：
  - texture ready
  - mesh ready
  - material ready
- 文档化 slot 稳定语义。

验收：

- 编译不破坏现有示例。
- 文档与 `ARCHITECTURE.md` 中 AssetHub / AssetTextureUploader 边界一致。

### Phase 1：Material 稳定 slot 接入主路径

目标：

- 复用现有 `MaterialManager` 的 slot / dirty / delayed reclaim 机制。
- 让 `GpuScene` 的 material buffer 数据来源从整场景 Vec 上传，迁移到 material manager。
- Material slot 在 register 到 unregister 生命周期内保持不变。
- Texture ready 后只标记对应 material dirty，不重建整个场景。

验收：

- 现有场景材质显示不回退。
- 多帧中同一个 material 的 slot 不变化。
- unregister 后 slot 延迟至少 `FIF_COUNT` 帧再复用。

完成记录（2026-05-17）：

- `RenderBackend` 新增过渡期 `MaterialBridge`，持有 `MaterialManager` 并维护
  `MaterialHandle -> ManagedMaterialHandle -> stable material slot` 映射。
- `SceneManager::prepare_render_data()` 改为通过 `MaterialSlotResolver` 输出
  instance 的稳定 material slot，不再构建整场景 material Vec。
- `GpuScene` 不再拥有 material buffer；`gpu::GPUScene.all_mats` 使用
  `MaterialManager` 当前 FIF material buffer 的 device address。
- `MaterialManager` 继续使用 fallback texture 策略，texture ready 后只标记相关
  material dirty，并通过调试日志记录 register / update / texture-ready / reclaim 路径。
- `PhongPass` 改为接收 `RenderData`，`truvis-render-passes` 不再依赖
  `truvis-world` / `truvis-scene`。

后续调整（2026-05-17 Phase 6a）：

- `MaterialManager` 已从 `truvis-scene` 移到 `truvis-render-backend` 私有模块，
  `ManagedMaterialHandle` / `TextureResolver` / `TextureBinding` 同步迁移到 render-side。

### Phase 2：Mesh AssetHub 管理 + AssetMeshUploader

目标：

- `AssetHub` 支持 mesh CPU 数据记录和加载事件。
- 新增 `AssetMeshUploader`，负责 vertex/index buffer 上传与 BLAS 构建。
- 移除新路径里的 `Mesh::build_blas_sync` 直接调用。
- `AssetMeshUploader::is_mesh_ready()` 成为 instance gate 的依据。

验收：

- Mesh 上传和 BLAS build 不阻塞 Assimp CPU 加载。
- BLAS 未 ready 时不会 panic。
- GPU resources 在 shutdown 时显式销毁。

完成记录（2026-05-17）：

- `AssetHub` 新增 `AssetMeshHandle` 对应的 CPU mesh 记录，通过
  `MeshAssetKey { source_path, mesh_index }` 做同一导入源内去重。
- `LoadedMeshData` 保存 positions / normals / tangents / uvs / indices / name，
  Assimp 导入阶段只把 C++ 临时 scene 数据复制到 Rust owned CPU buffer。
- `LoadedAssetEvent` 新增 `MeshLoaded`，`RenderBackend::begin_frame()` 将 asset 事件拆分给
  `AssetTextureUploader` 和 `AssetMeshUploader`。
- `AssetMeshUploader` 在 graphics queue 上提交 vertex/index buffer copy 与 BLAS build，
  使用 timeline semaphore 轮询完成；mesh ready 后提供 `MeshRenderResolver` 给
  后续 render-side scene bridge。
- 旧 `AssimpSceneLoader::load_scene()` 不再接收 GPU ctx，也不再同步创建 vertex/index buffer
  或调用 `Mesh::build_blas()`。
- `SceneManager` 中的 `Mesh` 变为轻量 proxy，只保存 `AssetMeshHandle` 和名称；mesh 未 GPU ready
  时，对应 instance 会被跳过，不写入本帧 `RenderData`。
- `GpuScene` 引入过渡期 TLAS revision；当 mesh uploader 有新 mesh ready 时，当前 FIF 的 TLAS
  会按 revision 重建。`RealtimeRtPass` 在当前帧 TLAS 尚未 ready 时跳过 ray tracing pass，
  避免启动早期帧 panic。

剩余限制：

- BLAS build 已异步提交，但第一版不做 compaction，后续可在 uploader 内补充 compact 流程。
- Assimp scene 文件读取仍是同步入口，尚未迁移到 `AssetHub::load_scene()`。
- Phase 2 完成时 TLAS revision 只覆盖 mesh ready 触发的粗粒度重建；Phase 3 已补上
  instance active/despawn/transform 的过渡期 revision，完整 dirty API 仍留给 Phase 4。

### Phase 3：Instance 稳定 slot 和 ready gate

目标：

- Runtime instance 引用 `AssetMeshHandle` 和 `AssetMaterialHandle`。
- `GpuScene` 为 `InstanceHandle` 分配稳定 `GpuInstanceSlot`。
- mesh/material 未 GPU ready 时 instance 保持 pending，不写入 active TLAS/draw list。
- 依赖 ready 后自动激活 instance。

验收：

- Instance 生命周期内 slot 不变化。
- Mesh/Material 未 ready 时不会访问无效 BLAS 或 material slot。
- transform 更新只 dirty 对应 instance slot。

完成记录（2026-05-17）：

- `truvis-asset` 新增 `AssetMaterialHandle`、`MaterialAssetKey` 和 `LoadedMaterialData`，
  `AssetHub` 负责 material CPU record、路径内去重和 CPU ready 状态。
- Assimp 同步入口保留，但导入结果改为先注册 `AssetMeshHandle` / `AssetMaterialHandle`，
  runtime `Instance` 直接引用 asset handles，不再依赖临时 `MeshHandle / MaterialHandle`。
- `RenderBackend` 新增 `InstanceBridge`，维护 `InstanceHandle -> GpuInstanceSlot` 稳定映射、
  pending/active 状态、FIF 延迟 slot 回收和 transform dirty revision。
- `MaterialBridge` 改为从 `AssetHub` 同步 `AssetMaterialHandle -> ManagedMaterialHandle -> stable material slot`，
  material ready 采用 fallback texture 策略，texture 未 ready 不阻塞 instance active。
- `RenderData` 的 active instance 携带 `GpuInstanceSlot`；`GpuScene` 按 slot 写入 instance buffer，
  raster push constant、TLAS `instance_custom_index` 和 RT shader instance 查询都使用稳定 slot。
- `InstanceBridge` revision 与 `AssetMeshUploader::ready_revision()` 合并为过渡期 scene revision，
  mesh ready、instance active/despawn/transform 会触发当前 FIF TLAS 重建检查。

剩余限制：

- `GpuScene` 仍整块上传 instance / geometry / indirect buffer，尚未按 dirty slot 做局部更新。
- TLAS 仍是 revision 触发的整棵 rebuild；Phase 4 已将该过渡机制正式作为当前 dirty/rebuild 策略。
- 旧 `MeshHandle` / `MaterialHandle`、`SceneManager::Mesh` / `SceneManager::Material`
  已在 Phase 6a 清理。
- Assimp scene 文件读取已迁移到 `AssetHub::load_scene()`，兼容 facade 已在 Phase 6a 删除。

### Phase 4：TLAS dirty/rebuild

目标：

- 引入 TLAS revision。
- instance active/despawn/transform/mesh ready 时标记 TLAS dirty。
- 当前 FIF 的 TLAS revision 落后时重建。

验收：

- 新加载 mesh ready 后能进入 ray tracing scene。
- despawn 后 TLAS 不再包含旧 instance。
- 不再依赖 “每个 FIF 第一次构建后永不更新” 的行为。

完成记录（2026-05-17）：

- `GpuScene` 为每个 FIF buffer 记录 `tlas_revision`；当前 FIF 的 TLAS 不存在或 revision 落后时重建。
- `AssetMeshUploader::ready_revision()` 与 `InstanceBridge::revision()` 合并为 render-side scene revision，
  覆盖 mesh ready、instance active/deactivate/despawn 和 transform dirty。
- TLAS `instance_custom_index` 使用稳定 `GpuInstanceSlot`，并保留 Vulkan 24-bit 上限检查。
- 当前帧 TLAS 为空时 `RealtimeRtPass` 跳过 ray tracing pass，避免 scene asset 或 mesh 尚未 ready 时 panic。

剩余限制：

- 当前策略仍是整棵 rebuild，不做 TLAS refit 或 dirty slot 局部更新。
- `GpuScene` 持有 TLAS 与 instance/geometry buffer 的 owner 边界已在 Phase 6c
  收敛到 backend 私有模块；dirty slot 局部更新仍留作后续。

### Phase 5：Assimp 读取集成到 AssetHub

目标：

- 新增 `AssetHub::load_scene(path) -> AssetSceneHandle`。
- 后台 Assimp task 只产出 owned CPU 数据。
- `AssetHub::update()` 分配 scene 内部 mesh/material/texture handles。
- World / SceneSpawner 根据 `AssetSceneHandle` 读取 scene asset，并通过 `SceneManager`
  创建 runtime instances。

验收：

- 旧同步 `AssimpSceneLoader::load_scene()` 可被新路径替代。
- `AssetSceneHandle` 能查询内部 mesh/material/texture handles。
- 同一 scene 多次 spawn 产生独立 runtime instances。

完成记录（2026-05-17）：

- `truvis-asset` 新增 `AssetSceneHandle`、`SceneAssetKey`、`LoadedSceneData` 和
  `LoadedSceneInstanceData`，scene asset 表示导入结果 / prefab，不持有 live `InstanceHandle`。
- `AssetHub::load_scene(path)` 通过 `AssetLoader` 后台 task 调用 Assimp FFI，task 内复制 owned CPU
  mesh/material/instance 数据并释放 `TruvixxSceneHandle`。
- `AssetHub::update()` 将 raw scene 数据转换为内部 `AssetMeshHandle` / `AssetMaterialHandle` /
  `AssetTextureHandle`，同帧发出 mesh upload 事件和 scene ready / failed 事件。
- `SceneManager::spawn_scene_asset()` 根据 `LoadedSceneData` 创建 runtime instances，同一 scene asset
  可多次 spawn 并产生独立 `InstanceHandle`。
- Cornell / Sponza demo 改为 init 阶段请求 scene asset、update 阶段 ready 后 spawn，不再直接调用旧同步 loader。
- `AssimpSceneLoader` 先降级为兼容 facade，不再持有 C++ FFI 导入逻辑；render-backend 去掉对
  `truvis-cxx-binding` 的直接依赖。Phase 6a 已删除该兼容 facade。
- C++ FFI 中 `truvixx_mesh_fill_tangents` 已修正为读取 tangent 数据。

后续调整（2026-05-17 Phase 7a）：

- scene material 中的相对 texture path 已在 `AssetHub` ingest scene 阶段按 scene 文件所在目录解析，
  绝对路径保持不变。
- texture path 只做词法归一化，不调用 `canonicalize`，因此纹理暂缺时仍走现有
  `load_texture` 失败和 render-side fallback 路径。
- 解析后的 texture path 继续通过 `AssetHub::load_texture()` 去重，同一归一化路径只分配一个
  `AssetTextureHandle`。

后续调整（2026-05-17 Phase 7b）：

- C++ Assimp importer 现在保存最近一次失败原因，并通过 C ABI 暴露
  `truvixx_scene_is_loaded()` / `truvixx_scene_last_error()`。
- `AssetLoader` 在 `truvixx_scene_load()` 返回非空 handle 后会先检查 scene 是否真的 loaded；
  加载失败时转为 `SceneFailed`，不再把失败导入当作空 scene 成功处理。
- 失败信息会带上 C++ importer 的错误字符串，便于定位格式错误、Assimp 解析失败等问题。

### Phase 6：清理旧路径

目标：

- 删除旧 `AssimpSceneLoader` 兼容 facade 和未使用的 loader 残留。
- 将持有 GPU buffer 的 `MaterialManager` 从 `truvis-scene` 移到 render-side。
- 清理旧 scene mesh/material 兼容身份和同步 mesh manager。
- 移除 `SceneManager::prepare_render_data()` 对临时 Vec index 的核心依赖。
- 将 `GpuScene` / `RenderData` 从 `truvis-render-interface` 上移到 renderer 集成层
  （Phase 6c 已落到 backend 私有模块）。
- 更新 `ARCHITECTURE.md` 和相关模块 README。

验收：

- 现有示例通过 `justfile` 中对应命令运行。
- `cargo fmt` / `cargo check` 通过。
- 文档描述与实际模块边界一致。

完成记录（2026-05-17 Phase 6a）：

- `MaterialManager`、`ManagedMaterialHandle`、`ManagedMaterialParams`、
  `TextureResolver` 和 `TextureBinding` 已迁移到 `truvis-render-backend` 私有模块；
  `truvis-scene` 不再持有 material GPU buffer owner。
- `truvis-scene` 删除旧 `MeshHandle` / `MaterialHandle` / `ManagedMeshHandle`、
  旧 mesh/material 组件存储、`mesh_manager` 和 shape 同步 GPU 创建路径。
- `floor` / `rect` / `cube` / `triangle` 已恢复为 CPU-only 程序化 mesh 数据，
  通过 `procedural_mesh` 输出 `LoadedMeshData` 和稳定 `MeshAssetKey`，用于辅助构建场景。
- `render-backend::model_loader` 旧 loader facade 已删除，scene 导入入口统一为
  `AssetHub::load_scene()` + `SceneManager::spawn_scene_asset()`。
- `truvis-scene` 去掉对 `truvis-gfx` 的依赖，暂时保留 `MaterialSlotResolver` /
  `MeshRenderResolver` 作为 scene 到 render-side 的过渡契约。

完成记录（2026-05-17 Phase 6b）：

- `MaterialSlotResolver` / `MeshRenderResolver` 已从 `truvis-scene::scene_manager`
  移到 `truvis-render-backend` 私有 `scene_bridge` 模块。
- `MaterialBridge` 和 `AssetMeshUploader` 实现 backend 内部 resolver trait，
  `InstanceBridge` 通过这些 render-side 契约构建 active `RenderData`。
- `truvis-scene` 已移除对 `truvis-render-interface` 的依赖，只保留 runtime
  instance / light 存储、asset handle 引用和 scene asset spawn 语义。
- `SceneManager` 不再通过 `MeshRenderResolver` 的返回类型引用 `RenderData`
  契约，scene 到 render-side 的过渡逻辑收敛到 backend。

完成记录（2026-05-17 Phase 6c）：

- `GpuScene`、`RenderData`、`GpuInstanceSlot`、`MeshRenderData` 和 `RtGeometry`
  已从 `truvis-render-interface` 迁移到 `truvis-render-backend` 私有
  `render_scene` 模块。
- `RenderBackend` 直接持有 backend 私有 `GpuScene`；`RenderWorld` 不再包含
  concrete GPU scene owner。
- `truvis-render-interface` 新增窄接口 `RenderSceneView`，render pass 只通过它读取
  scene buffer device address、当前 FIF TLAS handle 和光栅化 draw 能力。
- `GpuScene::upload_render_data()` 在 prepare 阶段维护当前 FIF 的 raster draw cache，
  `PhongPass` 不再接收 `RenderData`，`RealtimeRtPass` 不再直接访问 concrete
  `GpuScene`。
- `truvis-render-interface` 去掉 `gpu_scene`、`render_data` 和 `geometry` 公开模块，
  只保留帧调度、资源管理和 render pass 所需契约。

剩余限制：

- `GpuScene` 仍整块上传 instance / geometry / indirect buffer，尚未按 dirty slot 做局部更新。
- `MeshRenderResolver` 仍返回 backend 私有 `MeshRenderData`；后续若拆出独立
  render-scene crate，需要重新评估该私有契约是否上移。

### Phase 7a：Scene 纹理路径归一化

目标：

- 消除 Assimp scene 导入后 texture path 仍按原始字符串加载的问题。
- 相对 texture path 以 scene 文件所在目录为基准解析；绝对 texture path 不拼接 scene 目录。
- 保持 `AssetHub` 只负责 CPU asset 身份和加载状态，不引入 GPU 或 bindless 策略。

验收：

- `assets/fbx/sponza/sponza.fbx` 引用 `textures/albedo.png` 时，
  texture asset path 解析为 `assets/fbx/sponza/textures/albedo.png`。
- 绝对 texture path 保持原样。
- 两个 material 引用同一归一化 texture path 时复用同一个 `AssetTextureHandle`。

完成记录（2026-05-17）：

- `AssetHub` 在 scene material ingest 阶段解析 `diffuse_texture_path` 和 `normal_texture_path`，
  再调用 `load_texture()` 分配 / 复用 `AssetTextureHandle`。
- 新增私有词法路径归一化 helper，不访问文件系统，避免纹理文件暂缺时改变失败语义。
- `truvis-asset` 单元测试覆盖相对路径、绝对路径和归一化后 handle 去重。

剩余限制：

- embedded texture（如 Assimp 的 `*0`）暂未专项处理，仍按普通路径进入现有加载 / fallback 流程。

### Phase 7b：Assimp Scene 加载失败传播

目标：

- 修正 `truvixx_scene_load()` 失败时返回非空 handle 被 Rust 侧误判为 scene loaded 的问题。
- C++ importer 保存详细失败原因，并通过 C ABI 提供状态和错误查询。
- Rust asset loader 在复制 scene CPU 数据前显式检查加载状态，失败时产出 `SceneFailed`。

验收：

- 存在但内容非法的 scene 文件不会生成空 `LoadedSceneData`。
- scene load 失败日志 / 事件包含 C++ importer 返回的具体错误信息。
- 成功导入路径、texture fallback、GPU 上传和 scene spawn 流程不受影响。

完成记录（2026-05-17）：

- `SceneImporter` 新增 `last_error()`，文件不存在或 Assimp 解析失败时记录具体错误。
- C ABI 新增 `truvixx_scene_is_loaded()` 与 `truvixx_scene_last_error()`；
  `truvixx_scene_load()` 保留失败时返回 handle 的行为，方便调用方读取错误后释放。
- `AssetLoader::load_scene_task_inner()` 在 `TruvixxSceneGuard` 建立后立即检查 loaded 状态，
  失败时读取 importer 错误并返回 `SceneFailure`。
- 新增 `truvis-asset` 单元测试覆盖非法 scene 文件不会被当作成功导入。

剩余限制：

- embedded texture（如 Assimp 的 `*0`）暂未专项处理，仍按普通路径进入现有加载 / fallback 流程。

## 待确认问题

这些不是阻断设计的问题，已实施项保留结论，未实施项继续作为后续检查点：

1. `AssetMaterialHandle` 是否放在 `truvis-asset::handle` 中。
   已确认并实施，因为它表示内容资产身份。
2. `ManagedMaterialHandle` / `ManagedMeshHandle` 是否保留。
   已处理：`ManagedMeshHandle` 已删除；`ManagedMaterialHandle` 仅作为 backend 私有
   `MaterialManager` 内部身份保留，不再暴露在 `truvis-scene`。
3. Mesh 的 global geometry table 最终归属。
   推荐先由 `GpuScene` 过渡，最终迁移到 `AssetMeshUploader`。
4. Material texture readiness 默认策略。
   已确认 Phase 3 继续使用 Fallback，之后可支持 Strict。
5. C++ Assimp FFI 需要专项检查。
   已确认并修正 `truvixx_mesh_fill_tangents`，当前实现读取 tangent 数据。

## 风险

- 如果先做 `SceneHandle`，会把 CPU asset graph、GPU 上传、instance 生命周期和 TLAS dirty
  全部耦合在一次改动里，风险最大。
- 如果 `AssetHub::Ready` 被扩展成 GPU ready，asset/render 边界会再次混淆。
- 如果 Instance 激活不做 mesh/material ready gate，异步 BLAS 会继续触发无效访问或 panic。
- 如果 TLAS 不加 dirty/revision，异步加载完成后的 mesh/instance 不会稳定进入 ray tracing scene。
- 如果 draw path 继续使用临时 Vec index，就无法保证 instance slot 生命周期内稳定。

## 历史：推荐第一批实施任务

以下是路线起始时的第一批任务，当前已完成并保留作历史记录：

1. 补齐 handle / status 命名和文档。
2. 将现有 `MaterialManager` 接入主渲染路径，让 material slot 稳定。
3. 给 material slot 分配、dirty 上传、延迟回收增加 focused tests 或调试日志。
4. 更新 `ARCHITECTURE.md` 和模块 README。

完成后再进入 Mesh uploader 和异步 BLAS。这样每一阶段都有明确收益，也避免一次性重写 scene loading。
