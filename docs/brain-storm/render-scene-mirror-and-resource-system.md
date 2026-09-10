# RenderWorld 镜像与 ResourceSystem：状态对账和 GPU 同步设计

> 状态：2026-09-10 设计与首期实现已对齐。本文同时记录设计契约和实现边界；表格中的类型名以当前源码为准，伪代码不定义 Rust 或 shader ABI。真实运行结果以文末验证记录为准。

## 目标

将 Truvis 从“逐级传递变化原因”改为“复制渲染所需状态，由接收方比较最终结果”。保留 CPU 权威与 GPU 派生状态的边界，同时改变 RenderWorld 的数据组织和跨边界同步方式：

```text
ResourceSystem ──资源版本与内容──> RenderResourceSystem ──> GPU resource
World          ──场景状态对账──> RenderWorld          ──> GPU scene / TLAS
                                     ↑
                   RenderResourceSystem 的已发布资源视图
```

具体目标如下：

1. **降低变化维护成本。** World / ResourceSystem 的有效编辑只维护对象最终状态与 revision；不要求调用方同时维护 transform、material、TLAS、emissive 等传递规则。移除生产主路径对 `SceneChangeLog → DirtyEvent → DirtyRule → DirtyDispatchPlan` 的依赖。
2. **建立完整的渲染侧场景镜像。** RenderWorld 持久保存渲染所需的 instance、light、sky、资源引用与派生绑定。完成同步后，后端打包、TLAS、raycast 和 render pass 不再回读 World 或 CPU ResourceSystem。
3. **分开共享资源和场景组合。** ResourceSystem 管理 CPU 资源身份与内容；RenderResourceSystem 管理当前 device 的共享 GPU 资源；每个 World 对应自己的 RenderWorld，多个场景可以引用同一资源。
4. **接受必要的数据副本。** 允许复制 instance 状态、小型材质参数和 prepared 数据，以减少借用耦合和事件传播。共享资源按资源身份保存渲染副本，不按 instance 或 World 深拷贝 mesh / texture 大块内容。
5. **分别判断 GPU 更新与历史失效。** 材质外观变化无需借助 TLAS 重建清空累积；GPU 各 FIF 副本独立追平；运动历史按实际渲染帧推进。
6. **先建立简单且可验证的基线。** 第一阶段使用顺序 `update → prepare → render` 和全表版本扫描；后续按测量结果优化扫描与上传，不把完整 ECS、独立渲染镜像线程或通用事件系统作为前提。

成功的标志不是消除所有 dirty 或所有副本，而是新增一个普通材质字段时，只需修改权威数据、渲染投影与真正依赖它的消费者，不必补齐跨多层的事件枚举和路由规则。

### 简化原则

- 只保留一份 CPU ResourceSystem、一份 World 和一份持久 RenderWorld 镜像；允许必要的只读 snapshot，不复制完整 mesh / texture 大块内容。
- 不新增通用 `SceneDelta`、`ResourceDelta`、`GpuResourceDelta` 或必须可靠消费的 dirty 事件图；正确性来自最终状态、完整 membership 扫描和资源完成队列。
- 不为首期引入事务回滚、多 World 协调、独立 RenderWorld 线程、完整 ECS 或 per-property revision。
- mesh 与 texture 创建后不可变；不同内容通过新 handle 创建，避免同 handle 替换、旧内容并存和复杂的版本切换。
- material 只保留一个 CPU 权威状态和一个渲染侧 prepared snapshot；instance 只保存引用，不复制完整 material 内容。
- 先用全表扫描和整条 record 上传保证正确性，只有测量证明需要时才增加 changed-handle、chunk 或 sparse upload 加速。

### 首期支持范围

本文的目标设计只覆盖 Truvis 当前需要的静态场景与轻量编辑能力：

| 能力 | 首期契约 |
| --- | --- |
| scene 加载 | 通过 loader / importer 加载 FBX、glTF；GLB 及嵌入式纹理是否可用由具体 loader 明确声明 |
| 资源创建 | 创建 mesh、material，导入 texture；资源进入 CPU registry 后获得稳定 handle |
| instance | 创建 instance；保存 mesh 引用、material 引用和 transform |
| 可变编辑 | 修改 instance 的 transform 或 material 引用；修改 material 内容 |
| 不支持 | 不修改已创建 mesh 的顶点、索引或 submesh 内容；不做 mesh 内容热替换 |
| 引用查询 | 查询 material / mesh 被哪些 live instance 引用；texture 的 material 引用用于删除检查 |
| 删除 | 只允许删除没有 live 引用的 material、mesh、texture；删除后 GPU 资源延迟回收 |
| FIF | CPU scene / material 变化和异步资源 ready 都必须追平当前及后续 FIF 副本 |

“首期”默认一个 `RenderRuntime` 和一个 `World`。RenderResourceSystem 的共享边界保留未来扩展空间，但本设计不为多 World 引用协调增加额外协议。

首期只需要窄的同步 API，概念上可收敛为以下几组操作，不再为每种变化建立独立 command / event 类型：

| Owner | 最小操作 | 说明 |
| --- | --- | --- |
| ResourceSystem | `create_mesh`、`import_texture`、`create_material`、`update_material` | mesh / texture 创建后不可变；material 更新写回完整内容并推进 source revision |
| World | `create_instance`、`update_instance_transform`、`update_instance_materials` | instance 只保存引用和场景状态，修改后推进 instance source revision |
| World / SceneStore | `instances_using_material`、`instances_using_mesh` | 直接从反向索引回答引用查询和删除前检查 |
| ResourceSystem | `materials_using_texture`、`remove_orphan_*` | texture 依赖由 material / sky 反向索引检查；删除只接受无 live 引用资源 |

这些名称是职责示意，实际实现可以复用现有注册、更新和删除入口；重点是 API 写最终状态，prepare 再按 revision 对账。

## 当前基线

2026-09-09 在 `83faf876` 的工作区核对到以下实现。它们是迁移起点，不是目标约束。

| 方面 | 当前实现 | 设计约束 |
| --- | --- | --- |
| CPU owner | `World` 持有 `SceneStore` 与 `ResourceSystem`；资源内容、loader 和场景关系分边界保存 | ResourceSystem 保存资源，World 保存场景关系；导入器协调二者 |
| GPU owner | `RenderRuntime` 持有 `RenderResourceSystem`；`RenderWorld` 只持有场景 managers、buffer、TLAS 与历史 | 共享资源与场景组合分开，资源 manager 不属于某个 instance |
| 同步 | `World::poll_asset_loads` 只把 loader completion 写回 CPU registry；render prepare 由资源表全量对账 | 接收方读取最终状态，按对象版本、membership 和资源发布状态对账 |
| RenderWorld 数据 | instance manager 持久保存 handle、slot、transform/material 快照和历史；prepare 生成当前 `RenderData` | 渲染镜像自包含，RenderData 只作为本次打包视图 |
| 上传与失效 | 材质已有按 slot / FIF 的 dirty；instance / geometry / indirect 仍有全量重建、整容量 copy | 保留每个 GPU 副本的追平义务，逐步按实际内容与有效范围更新 |

首期实现已经移除生产路径中的 `SceneChangeLog` / dirty router，instance、material、mesh、texture 和 sky 通过最终状态扫描发现变化；材质参数和 texture ready 仍以各 FIF 的 dirty upload 保证副本追平。RenderWorld 对大块 scene buffer 使用独立的 per-FIF scene revision，静态副本追平后不再重复复制；scene root 仍每帧写入当前绑定快照。

阅读入口：

- [`scene-data-lifecycle.md`](../summaries/scene-data-lifecycle.md)：当前 scene / asset 身份和同步主线。
- [`frame-lifecycle.md`](../summaries/frame-lifecycle.md)、[`threading-and-resource-lifecycle.md`](../summaries/threading-and-resource-lifecycle.md)：帧边界、线程与 GPU 生命周期。
- [`resource_system.rs`](../../engine/e30-world/truvis-world/src/resource_system.rs)、[`scene_store.rs`](../../engine/e30-world/truvis-world/src/scene_store.rs)：CPU 资源与场景 owner。
- [`render_resource_system.rs`](../../engine/e40-render/truvis-render-runtime/src/render_world/render_resource_system.rs)、[`render_instance_manager.rs`](../../engine/e40-render/truvis-render-runtime/src/render_world/render_instance_manager.rs)、[`render_world.rs`](../../engine/e40-render/truvis-render-runtime/src/render_world/render_world.rs)：GPU 资源 owner、场景镜像与打包。

## 待推进内容

### 1. 设计选择与参考

选择“持久镜像 + revision pull”：prepare 对账当前状态，变化结果由数据消费者判断。保留 loader / upload completion 队列，取消把场景影响串成通用事件图的要求；不为本项目引入新的 delta 层级或通用规则注册表。

| 方案 | 收益 | 代价与结论 |
| --- | --- | --- |
| 修补现有 router | 改动较小 | 不能解决 RenderWorld 数据组织与跨层传播耦合，只作为迁移中的修复 |
| 持久镜像 + revision pull | 同步自包含、修改合并自然、可从最终状态恢复 | 增加 CPU 镜像和扫描成本；作为本设计基线 |
| 完整 ECS / 跨线程 extract pipeline | 能支撑独立调度与更多并行 | 生命周期和调度改造较大；没有当前需求支撑，不作为前置条件 |

其他引擎提供的是边界设计参考，并不都使用本设计的全表 pull 算法：

| 参考 | 借鉴内容 | 不直接照搬的部分 |
| --- | --- | --- |
| RenderV2 | GameWorld 投影到可查询的 RenderWorld；后端消费渲染域数据，共享材质使用资源身份 / MaterialID | 当前 typed source message、ECSStagingWorld 和调度器有其并行约束，不代表消息链天然更简单 |
| Bevy | MainWorld 经 Extract 进入 RenderWorld；资源有 extracted / prepared 表示 | change detection、asset event 和 ECS extraction 仍有同步机制，不等同于取消变化跟踪 |
| UE | GameThread component 与 RenderThread scene proxy 分开，proxy 保存渲染所需状态 | render command queue 服务其线程模型，本设计暂不引入同样的跨线程命令协议 |
| Unity | BRG 将共享 mesh/material 与 instance buffer、绘制批次分开 | 传统 GameObject、Entities Graphics 与 BRG 的边界不同，不统一描述成两个完整 ECS World |

RenderV2 的本地对照入口为 `d5enginev2/engine/modules/render/renderer/README.md` 和同模块的 `render_v2/render_entity_components.hpp`。外部参考仅用于上述高层边界：Bevy 的 [ExtractSchedule](https://docs.rs/bevy/latest/bevy/render/struct.ExtractSchedule.html) 与 [RenderAsset](https://docs.rs/bevy/latest/bevy/render/render_asset/trait.RenderAsset.html)、UE 的 [Threaded Rendering](https://dev.epicgames.com/documentation/unreal-engine/threaded-rendering-in-unreal-engine?lang=en-US)、Unity 的 [BatchRendererGroup](https://docs.unity3d.com/cn/6000.0/ScriptReference/Rendering.BatchRendererGroup.html) 与 [创建批次](https://docs.unity3d.com/cn/6000.0/Manual/batch-renderer-group-creating-batches.html)。

### 2. 所有权与依赖方向

```text
RenderRuntime
├── ResourceSystem
│   ├── resource identity / CPU content / source revision
│   ├── material -> texture dependencies
│   └── AssetHub / load coordinator
├── World
│   ├── instance / transform / mesh-material references
│   ├── light / sky state
│   └── instance membership / source revision
├── SceneAssetImporter / CPU edit coordination
├── RenderResourceSystem
│   ├── prepared material snapshots / stable material slots
│   ├── texture image / view / bindless residency
│   ├── mesh vertex-index buffers / BLAS / geometry metadata
│   ├── upload-build completion / published resource view
│   └── GPU resource retirement
├── RenderWorld（每个 World 一份）
│   ├── render instance / light / sky mirrors
│   ├── used mesh-material bindings / stable instance slots
│   ├── current-previous transform history
│   ├── raster data / raycast records / indirect layout
│   ├── emissive table / TLAS
│   └── GPU scene replicas / scene history versions
├── GfxResourceManager
└── frame timing / FIF / RenderGraph / command submission
```

这是逻辑 owner 划分，不要求同时创建同名 crate 或抽象每个列表项。

- **ResourceSystem** 是 material、mesh、texture 的唯一 CPU 内容权威，管理身份、依赖和加载状态。mesh 与 texture 在创建后内容不可变；material 参数可以修改并推进 source revision。它不知道 RenderWorld 的 TLAS 或 Vulkan 对象。
- **World** 是场景关系权威，只保存资源引用和 instance / light / sky 状态。共享材质修改影响所有引用者；只改一个 instance 的材质时，选择另一个 material handle 或显式复制资源，再修改引用。
- **SceneAssetImporter / 编辑协调器** 通过 ResourceSystem 与 World 的接口完成注册和引用验证。导入器是协调者，不持有第二份长期资源库，也不创建 Vulkan 对象。
- **RenderResourceSystem** 按 resource handle 和 device 管理首次安装、准备、发布及回收。BLAS 与 mesh buffer 同属不可变 mesh GPU resource；材质 GPU slot 不属于某个 instance 或 World。
- **RenderWorld** 是可重新生成的渲染镜像 owner，保存场景组合和历史；TLAS 属于它。资源完成处理不直接修改它的对象表。
- **GfxResourceManager** 保持底层 buffer / image 分配与释放契约。高层 owner 决定何时退役，底层执行安全释放；二者不是两份独立的 Vulkan 所有权。

固定访问方向为：

```text
SceneAssetImporter / CPU edit coordination -> ResourceSystem + World
RenderResourceSystem::sync                -> ResourceSystem::ResourceView
RenderWorld::sync                         -> World::SceneView
RenderWorld::resolve                      -> RenderResourceSystem::PreparedResourceView
backend packing / TLAS / render passes    -> render-side mirrors and bindings
```

同步读取结束后，不在 RenderWorld 中保留指向 World / CPU ResourceSystem 的借用。PreparedResourceView 提供材质语义、mesh 几何元数据及 GPU 绑定；emissive、TLAS 和上传代码不能为补字段绕回 CPU owner。

第一阶段仍在现有 RenderThread 上顺序执行 update / prepare / render，不改变 `app -> renderer -> engine` 依赖，也不引入全局单例或 `Arc<RwLock<ResourceSystem>>`。未来真正并行化时，需要单独设计不可变发布快照及背压；拥有镜像本身不代表已经具备线程安全协议。

### 3. 复制范围与 RenderWorld 数据组织

“完整镜像”指渲染所需信息完整，而不是复制整个游戏世界、编辑器状态或所有资源字节。

| 数据 | CPU 权威 | 渲染侧副本 | 共享方式 |
| --- | --- | --- | --- |
| instance transform、mesh/material 引用、可见性语义 | World | RenderWorld 持久实例记录 | 每个场景一份；不包含每个 view 的剔除结果 |
| material 参数与纹理引用 | ResourceSystem | RenderResourceSystem 按材质保存小型 source snapshot、prepared 参数与绑定 | 同 device 多个 RenderWorld 共享；不按 instance 复制完整参数 |
| material 对场景的派生信息 | 无独立权威，由上述数据产生 | RenderWorld 按使用的 material 保存 slot、coverage、emissive 与已观察版本 | instance 持有引用或索引 |
| mesh / texture 大块 CPU 内容 | ResourceSystem | 上传期间可持有不可变 payload lease | 第一阶段保留一份 CPU 内容；允许不可变共享，禁止每个 RenderWorld 深拷贝 |
| mesh buffer、BLAS、image、descriptor | 不属于 CPU World | RenderResourceSystem | 按资源和 device 共享，引用带代际 / 生命周期保证 |
| light、sky 选择与强度 | World | RenderWorld 镜像与场景绑定 | HDRI image 及可共享的资源派生数据由资源层管理 |
| GPU instance/material/indirect 数据 | 从渲染镜像派生 | 各 GPU FIF 副本 | 由 buffer owner 独立追平 |

CPU 大资源的提前卸载是后续策略；若释放 CPU bytes，ResourceSystem 必须仍掌握身份、内容版本和重载来源。这个策略不把权威内容转交给 RenderWorld。

RenderWorld 内部采用带 generational handle 的持久表与可复用数组，不要求先引入 ECS。概念形状如下，字段是职责示意，不定义 Rust / Slang ABI：

```text
RenderInstanceRecord
    source_handle + source_revision_seen + membership_seen_epoch
    source_snapshot { mesh_handle, material_handles[], transform, enabled, ... }
    resolved_mesh { ready generation, geometry ranges, BLAS binding }
    resolved_materials[] { material handle, slot, dependency stamps }
    stable_instance_slot + pending/active state + requires_any_hit
    prepared_transform + previous_transform + last_submitted_transform

PreparedMaterial（RenderResourceSystem 中每种材质一份）
    source_revision_seen + copied render-relevant material parameters
    texture_dependency_stamps[] + resolved bindings
    stable_material_slot + packed GPU material
    prepared_revision + appearance_revision + per-FIF upload state
```

一个 instance 可能引用多个 material，材质也可能引用多个 texture；依赖 stamp 必须逐项保存，不能用一个 `resolved_material_revision` 比较多个不相关资源，也不能将版本求和或取最大值替代身份与版本匹配。

实例表、材质引用表和 topology 数组是持久数据。RasterDrawData、raycast 反查与 indirect layout 按结构变化重建；每个 view 的 culling/draw selection 可另行生成。RenderData 最终只提供这些表的只读切片或视图，不再额外长期保存一套可写对象事实。

### 4. 版本契约与变化发现

版本是接收方判断“是否需要重新读取”的依据，不是按顺序回放的事件历史。

| 标识或版本 | Owner | 推进条件 / 用途 |
| --- | --- | --- |
| handle generation | CPU registry / World | 区分删除后复用索引的新对象；不能由 GPU slot 替代 |
| source revision | CPU material / instance / light / sky 记录 | 有效编辑提交后推进；mesh / texture 内容创建后不再修改；失败编辑与相等赋值不推进 |
| membership / table epoch | CPU 容器，可选优化 | 增删或表内容变化时用于跳过已确认无变化的扫描，不是正确性的唯一依据 |
| published resource / binding revision | RenderResourceSystem | 首次发布 texture / mesh ready、BLAS ready 或 material prepared binding 时推进；不把 CPU ready 当 GPU ready |
| prepared revision | 对应渲染数据 owner | 最终待上传内容或其资源绑定发生变化时推进，包括没有 CPU 编辑的依赖 ready |
| uploaded revision[FIF] | 各 GPU buffer owner | 对应副本已安排并提交了该版本的有序更新；GPU 完成另由 timeline 判断 |
| scene history versions | RenderWorld / 历史消费者 | 实际外观、几何或采样语义变化时推进，不跟随 FIF 补写重复推进 |

一个 revision 只在同一对象、同一代际、同一语义域内比较。初版每个 CPU 对象使用一个 source revision；不为每个属性发明独立版本。Source snapshot 中的名称等 metadata 变化可以触发一次重新读取，但渲染投影相等时不推进上传或历史版本。

CPU 修改在验证成功后一次性写入完整状态并推进 revision。若支持可变访问 guard，也必须在 guard 提交时完成同样的契约；不能暴露绕过版本维护的任意可变引用。

初版对 ResourceSystem 的注册表和 World 的完整 scene membership 做扫描。复杂度约为 `O(R + N + B)`：R 为资源记录数，N 为 instance 数，B 为实际检查的引用总数；不能把多材质依赖检查都算成常数成本。资源源数据和派生打包只在版本或依赖变化时复制 / 重算。

现有 1024 instance 上限仅是当前实现背景，不构成性能证明或目标上限。后续可以用 chunk version、dirty bitset 或 changed-handle 集合跳过扫描；这些只能是候选对象加速索引，清空后做全量对账仍应得到相同结果，不能重新引入必须可靠消费的语义 changelog。

### 5. 对账算法与完整性

每次 prepare 建立一个稳定观察边界，先收敛 CPU 编辑和 loader 完成，再发布资源状态，最后对账场景。首期不要求 scene import 具备事务回滚；loader 失败只需报告失败，已经注册的 mesh、material 或 texture 作为普通 CPU 资源保留，若无引用即可按孤儿资源删除。

**资源对账：**

1. 比较 CPU registry membership 与 render-resource records，处理删除和已失效的首次上传请求。
2. 扫描 CPU ResourceStore 中的不可变 mesh/texture 内容，为尚未安装的资源提交上传；比较 material source revision 和 texture binding revision，准备新的 material snapshot。mesh / texture 没有内容更新路径。
3. 即使 CPU revision 未变，也继续推进 upload / BLAS / sky distribution 等异步工作并检查完成。
4. 按纹理、mesh 等依赖更新 prepared material 与资源元数据，发布一次一致的 PreparedResourceView。

**场景对账：**

```text
begin sync_epoch
for each instance in complete World membership:
    find or create mirror by generational handle
    mark mirror seen in sync_epoch
    if source revision differs:
        copy final render-relevant source state
    compare current resource stamps even if source revision did not change
    resolve mesh/materials and pending/active state
    compare old/new TLAS input, topology and light/appearance semantics
    update persistent mirror and accumulate local output changes
retire mirrors not seen in this complete scan
sync light and sky mirrors
finalize derived tables and pending GPU revisions
```

完整扫描必须包含隐藏、Pending、未 GPU-ready 的 instance，不能以视锥可见列表充当 membership。只在完整扫描成功后执行未见对象删除；若将来引入分块快照，必须等 membership 完整才做 sweep。

这种方式自然处理增删：新增对象没有镜像则创建；删除对象不再出现则退役；一次同步前创建又删除的对象从未进入镜像。扫描以 handle generation 为身份，不会将复用索引的新对象当作旧对象。暂时无需 created / removed 历史队列或专门抵消规则。

资源相同、source revision 相同也不代表可以跳过整次 prepare：资源完成、FIF 补写、运动历史和 retirement 都有独立推进需求。全局 scene revision 最多跳过 CPU source copy，不能屏蔽这些工作。

对账输出可保留一个局部 `RenderWorldChangeSummary`，例如 instance 更新区间、topology/TLAS/emissive 的变化和 appearance invalidation。它只承接这次计算的结果，不再转换成另一组事件供规则解释。GPU 待更新义务必须保存到 buffer owner 的版本状态中，不能随临时 summary 销毁。

资源层保留 `material -> texture` 依赖查询，World 保留 mesh/material 到实例的关系以提供查询和删除检查。初版不要求通过这些索引推送 transitive dirty；RenderWorld 读取 PreparedResourceView 即可发现资源 ready 或 material prepared 版本变化。除 loader / upload completion 外，不再新增 `SceneDelta / ResourceDelta / GpuResourceDelta` 三套变化对象。

### 6. 资源身份、就绪与删除

CPU 内容状态和 GPU 安装状态分属不同 owner，不共用一个 Ready 枚举：

```text
CPU: registered -> loading -> ready | failed
GPU: absent -> uploading -> buffer/image ready -> published
                               mesh 另有 BLAS building -> BLAS ready
```

Material 参数可先使用 fallback texture；mesh 的 raster buffer ready 与 BLAS ready 是独立能力。初版保持现有主要 RT 路径的 ready gate：所需 BLAS 和 material slot 可用后才进入 Active。分开状态不等于本阶段必须实现 raster/RT 两套 active population。

- **首次加载：** texture 未 ready 时使用 fallback；mesh 不满足门禁时保留 Pending 镜像，不输出无效 TLAS instance。
- **不可变内容：** mesh 与 texture 在注册成功后不提供内容修改或同 handle 替换。需要不同 mesh 或 texture 时，创建新 handle，再修改 material 或 instance 的引用；旧 handle 在无引用后删除。
- **发布完整性：** mesh 几何范围、triangle metadata 和 BLAS 属于同一次首次发布；material 的 texture binding 只指向已经发布或明确的 fallback。
- **迟到完成：** 首次上传请求携带 resource handle generation。已删除 handle 的完成结果只能进入回收，不能重新发布 shader-visible binding。
- **完成队列：** loader / upload / build completion 是任务事实，保留队列；它们由所属 owner 消费，更新状态表。RenderWorld 不需要再次订阅这些队列。

显式删除 CPU resource 前必须验证引用。material 被 instance 引用时不能删除，mesh 被 instance 引用时不能删除，texture 被 material 或 sky 引用时不能删除。删除被拒绝时，不修改内容、引用和版本。首期只有一个 World，引用检查直接由 World / SceneStore 的反向索引完成；不为多 World 增加协调器。

CPU 删除与 GPU 释放分开：实例镜像在对账时退役，旧 slot、BLAS、image 和 descriptor 要等待引用它们的在飞提交安全结束后才能回收或复用。CPU handle generation、GPU slot generation 和 GPU completion 是不同约束；某一帧的 FIF wait 不能被当作所有异步任务都已结束。

停止运行时，先停止接收 CPU 编辑和新资源请求，再退出场景使用、处理未完成任务和 GPU 提交，释放 RenderWorld 场景资源、RenderResourceSystem 共享资源，最后释放底层分配器和 device。逻辑树调整不能绕过现有显式 shutdown 契约。

### 7. Prepare、GPU 副本与运动历史

```text
begin_frame
    wait current FIF reuse boundary / reclaim safe retired objects
update
    commit ResourceSystem + World edits
prepare
    collect loader results / finish CPU scene import ingest
    RenderResourceSystem sync + poll + publish prepared resources
    freeze resource view for this prepare epoch
    RenderWorld sync source mirrors + resolve bindings
    derive topology / temporal payload / TLAS / light changes
    publish descriptors before recording dependent material/scene use
    record current FIF resource and scene updates + required barriers/builds
    finalize scene root + scene history signature
render
    consume only this prepared render-side view
submit
    commit upload bookkeeping and rendered-frame history
```

上面固定的是依赖顺序，不要求所有 GPU 工作合并成一个函数或一次 submit。Texture/BLAS 是否可发布依赖实际 completion 或明确的队列等待；把命令入队不等于 GPU 已完成。prepare 中途不能异步改写同一视图内的 descriptor、material snapshot 或 BLAS。

GPU buffer owner 使用 `target prepared revision` 和每个 FIF 的 `uploaded revision`。A/B/C 可以分别从旧版本直接追到最新版本，不需要保存中间编辑历史。材质目标版本必须覆盖其纹理绑定变化，不能只使用 material CPU source revision。

记录 copy 后、提交前的 bookkeeping 是暂定状态：如果本帧取消或提交失败，不能把它当作已提交的新副本。成功提交表示 copy/build 与后续消费已建立顺序，资源回收仍等待 GPU completion。Buffer 重新分配或 device 重建时，相应 uploaded 状态失效。

初版允许整表或整条 record 上传，先保证版本正确；优化时优先复制有效区间并合并相邻 slot。Instance slot 有空洞，使用跨度不是 live count；结构重排必须同时更新 geometry/material indirect、emissive base map、draw 与 raycast 引用。空场景必须发布空 count / 无 TLAS，不能因为没有 payload 而继续暴露旧根数据。

运动历史由 RenderWorld 拥有，定义为“上一实际参与该渲染历史的已提交场景状态”，不是上一 CPU update，也不是当前 FIF 三帧前的值：

- 本帧 `current = 最新镜像 transform`，`previous = last_submitted_transform`。
- 首次激活和 history reset 时令 previous 等于 current，避免虚假运动。
- 成功提交参与该历史的场景后，再推进 last_submitted_transform；跳过渲染、取消 prepare 或失败提交不推进。
- 移动后静止的下一渲染帧仍需写入 previous=current，即使 World 没有新编辑。
- 一个 RenderWorld 可以被多个 view 读取；view 的剔除结果不写回场景镜像，场景历史只由本 RenderWorld 的实际提交推进。

因此 temporal payload 变化可以推进 instance upload revision，但不能因此无条件推进 TLAS 或外观语义版本。FIF 中仍存旧 previous 的副本，也必须在下次使用前追平。

### 8. 由最终派生输入决定更新与历史失效

消费者比较自己真正依赖的字段。比较语义字段或已定义的规范化 key，不比较含未定义 padding 的原始内存；CPU 渲染镜像也不另行定义一套与生成 shader binding 重复的 ABI。

| 实际变化 | GPU / 派生更新 | 历史处理 |
| --- | --- | --- |
| 仅名称等 metadata | 重新读取后可不更新任何 GPU 内容 | 不清除渲染历史 |
| roughness / metallic 等着色参数 | material payload | appearance 失效，不要求 TLAS 或 emissive 表重建 |
| 普通 texture fallback -> ready | prepared material / descriptor 绑定；即使 slot 数字未变也观察资源 ready generation | appearance 失效 |
| cutoff 改变但 requires_any_hit 未变 | material payload | appearance 失效，TLAS flags 不变 |
| instance 最终 requires_any_hit 改变 | TLAS 输入 / flags | 几何可见性与相关历史失效 |
| material 引用列表改变 | 引用与 indirect 布局；按实际结果判断 TLAS flags / emissive | 按最终外观和结构判断 |
| 当前 transform、BLAS ready 或 Active 集合改变 | instance / TLAS；有相关贡献或布局依赖时更新 emissive | 对应场景与采样历史失效 |
| 只有 previous transform 推进 | temporal instance payload | 不视作新的 scene edit 或 TLAS 输入变化 |
| emissive 分类、辐射、权重或 record 布局改变 | light records / alias / base map 中真正受影响的部分 | emissive 采样版本与 appearance 按需变化 |
| analytic light / sky 状态或 HDRI 发布变化 | 对应场景灯光、环境绑定 / 分布 | 对应光照与外观历史失效 |

TLAS 输入 key 包括 stable instance index、transform、BLAS 发布代际及实际 flags 等；material slot 值本身不是 TLAS descriptor 的输入。共享 material 修改只需更新一次 prepared material，不应给所有引用者伪造“材质列表被编辑”。实例可以因观察到材质语义变化而重新计算 requires_any_hit，但只有结果变化才更新 TLAS。

Emissive 表还依赖 active instance / submesh 的索引布局；增删非发光对象也可能影响 base map，不能仅检查 is_emissive。后续若优化采样权重，还需覆盖它实际读取的纹理内容和参数。

RenderWorld 单独观察当前场景实际使用的资源 appearance revision，建立场景 appearance 版本；未被该场景使用的资源变化不应强制清空它的历史。离线 accumulation signature 显式包含外观影响，不再借用 TLAS / emissive 重建传达普通材质变化。实时 temporal / ReSTIR / SHARC 等消费者保持各自的兼容性策略，不把“外观变化”机械等同于清空一切缓存。

CPU source revision、GPU prepared/upload revision、TLAS 输入版本、emissive 采样版本和 appearance 版本不能合并成一个全局数字。尤其 A/B/C 为同一版本补写时，只更新副本状态，不反复制造场景语义变化。

### 9. 首期操作与成本

首期只有一个 `RenderRuntime` / `World`。RenderResourceSystem 在该 runtime 内共享资源 GPU 状态，RenderWorld 保存这一场景的 instance slot、TLAS、raycast 与历史；不为跨 World 的引用计数、同步广播或独立 device 增加抽象。多个 view 仍可读取同一个 RenderWorld，但 view 的剔除结果不写回场景镜像。

| 操作 | 对账行为 |
| --- | --- |
| FBX / glTF 导入 | loader 输出 CPU 数据；导入器注册资源并创建 World instance；prepare 观察新 membership，资源未 ready 时保存 Pending 镜像。导入失败不做 rollback，留下的无引用资源可删除 |
| 同一次同步中移动并换材质 | source revision 推进，镜像读取最终 transform 与完整 material 列表；两项分别比较，不用强度枚举互相覆盖 |
| CPU 修改 A -> B -> C | 接收方直接复制 C，不回放 B；若最后回到已观察的 A，投影相等则无需无意义重建 |
| World 无编辑但 texture / BLAS ready | 资源 view 的发布状态变化；材质或实例对账发现新 ready generation，更新外观或激活状态 |
| 实例在同步前创建又删除 | 不出现在完整 membership 中，不分配渲染镜像或 GPU slot |
| instance 删除或场景清空 | mirror sweep 退役场景记录；无引用的 CPU resource 可删除，GPU resource 仍按 completion 延迟回收 |

额外内存主要是 instance source mirror、资源依赖 stamps、小型材质副本及持久 topology/raycast 数据，规模约 `O(N + B + M)`，M 为需要 prepared 的材质数；GPU FIF 副本原本就有独立生命周期，不归因于本次 CPU 镜像新增成本。大 payload 的 CPU 保留策略需要单独计量。

实现中保留静态场景的廉价 revision/membership 扫描；材质与资源只在 source/binding revision 变化时重算，GPU FIF 副本按各自 dirty 状态补写。当前没有加入 chunk version / bitset；后续是否增加必须以扫描、copy bytes、TLAS build 和 prepare 时间测量为依据，本设计不承诺未经测量的性能提升。

### 10. 迁移顺序

目标架构先固定，文件移动、owner 拆分和行为替换分别形成可验证步骤：

1. **建立 source revision 与持久镜像。** 已完成：有效编辑推进最终状态和 revision，RenderWorld 做完整 membership 对账，RenderData 从镜像生成。
2. **收窄 CPU 资源与场景。** 已完成：`ResourceSystem` 承接不可变 mesh/texture、可变 material 与依赖，`SceneStore` 只保存 instance/light/sky 关系。
3. **建立 RenderResourceSystem 发布边界。** 已完成：texture/mesh/material/sky manager 和 upload queue 由 runtime 级 `RenderResourceSystem` 持有，支持首次上传、ready/fallback、material 更新和安全回收。
4. **切换 revision pull 主路径。** 已完成：生产路径不再依赖 DirtyEvent / DirtyRule / DirtyDispatchPlan；全表扫描是正确性基线。
5. **独立历史与 GPU 副本更新。** 首期已完成 FIF dirty 补写、mesh/texture late completion 防护、静态 scene buffer revision 和 FIF 延迟回收；appearance history 使用独立版本，不借 TLAS 变化传递普通材质失效。
6. **按 profile 优化。** 未纳入首期；TLAS refit、异步 TLAS build、staging thread、mesh/texture 内容热替换与跨线程快照继续保持非目标。

每一步保持可渲染且能独立验证。当前实现摘要和模块 README 已同步；相邻的 [`asset-upload-and-scene-evolution.md`](asset-upload-and-scene-evolution.md) 只承接上传调度、失败恢复与资源卸载的后续能力，不再定义竞争的 scene 传播模型。

## 边界与非目标

- 唯一 CPU 权威、单向投影和 Vulkan 的 RenderThread 生命周期保持不变；被改变的是资源归属、render-side 数据组织与同步算法。
- 首期 scene 导入不要求事务回滚。导入失败只需让 import 状态失败并保留已注册资源；这些资源若没有 live 引用，应可以通过孤儿删除接口清理。
- mesh 与 texture 内容在创建 / 导入后不可修改；不支持同 handle 顶点、索引、像素内容更新或热替换。不同内容使用新 handle。
- “允许副本”不允许多个可独立编辑的材质真相；RenderWorld / RenderResourceSystem 的 CPU snapshot 均可从权威状态重建。
- 不承诺移除 completion queue、GPU retirement、FIF tracking 或历史版本。这些处理不同生命周期，不能被一个全局 scene_dirty 取代。
- 不为取消 changelog 引入另一套必须可靠消费的资源 / 场景事件协议。源版本、完整 membership 和资源发布状态必须足以恢复镜像；loader / upload completion 队列仍然保留。
- 第一阶段不实现完整 asset database、网络复制、事件回放、撤销系统、通用 ECS scheduler、独立 RenderWorld 工作线程、多 World 协调或多 device 渲染。
- 本文中的记录布局不是 GPU ABI；共享 shader 结构仍从既有 ABI owner 生成，真正更改布局时按项目 ABI 规则验证。

## 完成标准

### 架构与维护

- ResourceSystem、World、RenderResourceSystem、RenderWorld 的 owner 和只读接口已建立；不可变 mesh / texture、可变 material 与场景 instance / TLAS 分离。
- RenderWorld 持有自包含的 render scene 镜像；后端打包、TLAS、灯光表和 render pass 不回读 World / CPU ResourceSystem。
- 有效 CPU 编辑只维护最终状态与 source revision；普通新增字段无需扩展跨层事件规则。
- 生产正确性不依赖旧 changelog/router，也不依赖 optional changed-handle 队列的可靠消费；全量对账可重建一致的镜像。
- 明确记录 CPU 镜像内存、扫描与上传成本；性能声明有数据支持，功能正确不等于性能已验证。

### 行为验证场景

| 场景 | 应观察到的结果 |
| --- | --- |
| 同步前连续修改 / 相等赋值 / 编辑失败 | 只观察最终有效状态；失败或相等赋值不制造版本变化 |
| 同时移动与换材质 | current、previous、binding 均正确；静止下一帧运动归零 |
| 隐藏或 Pending instance | 不被 membership sweep 误删；资源 ready 后可正确激活 |
| 同步前创建又删除 / handle 索引复用 | 无多余镜像；新代际不继承旧绑定或历史 |
| 共享材质被多个 instance 引用 | 资源更新一份，所有引用 instance 都观察到最终材质；无先消费丢变化 |
| CPU 无编辑而 texture / BLAS ready | 外观更新或 instance 激活，相关历史正确失效 |
| 导入失败与无引用资源 | import 状态失败；已注册但无引用的资源可被查询并删除 |
| 删除被引用资源 / 上传途中删除 | 验证、退役和 completion 各自正确；无资源复活 |
| A/B/C 落后程度不同，期间继续编辑 | 每份 GPU 副本使用前追到最新目标；补写不重复清空历史 |
| prepare 后取消、提交失败、跳过渲染 | 不虚报上传成功，不错误推进 last_submitted_transform |
| 仅名称、roughness、cutoff、coverage 改变 | 各自只更新实际受影响的数据和历史；无借 TLAS 传递普通外观失效 |
| 增删非发光对象 / 空场景 / 稳定 slot 有空洞 | indirect、base map、raycast、TLAS 和 root count 保持一致 |
| 静态场景及 buffer 重建 | 静态副本追平后不重复上传；新 buffer 不继承旧 uploaded 状态 |

验证应区分 CPU 对账检查、提交/生命周期检查和真实 GPU 画面结果。首期已执行的检查如下：

| 检查 | 结果 |
| --- | --- |
| `cargo test -p truvis-world` | 4 项通过，覆盖 mesh 约束、instance/material 数量校验和 material/mesh/texture 孤儿引用检查 |
| `cargo check -p truvis-render-runtime -p cornell-renderer -p cornell-app` | 通过 |
| `just cornell` | 构建并启动到渲染循环；日志观察到 FBX 导入 11 个 instance、sky texture 与 distribution 发布；未覆盖编辑器连续修改、删除或长时间 FIF 压力 |

应用构建或启动日志只能证明进程路径走通，不能替代逐像素画面验收；GPU 画面回归仍需单独补充。
