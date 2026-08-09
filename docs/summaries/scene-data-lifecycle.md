# CPU Scene 到 GPU Scene 同步机制

> 状态：当前实现事实总结。本文说明 `truvis-world` 与 `truvis-render-runtime` 之间的 scene 同步机制：
> 硬盘或运行时生成的 CPU 数据如何进入 `World`，再在 prepare 边界变成 render-side GPU scene。

## 机制定位

这套机制的核心不是“资产库直接喂给 GPU”，而是两段翻译：
`AssetHub` 把硬盘文件变成 owned CPU payload，`World` 把 payload 变成 CPU scene 语义；
`RenderWorld` 再把 CPU scene 语义变成 shader 可读取的 GPU cache、buffer、bindless handle 和 TLAS。

`World` 是 App / Plugin 在 update 阶段面对的 CPU 语义入口。它内部持有 `SceneStore`、
`AssetHub` 和 `SceneAssetIngestor`，但不持有 Vulkan image、buffer、BLAS、TLAS 或 GPU slot。
`RenderWorld` 是 `RenderRuntime` 内部的 render-side prepared world，持有 texture / mesh /
material / instance / sky / emissive / TLAS 等 GPU 派生状态。

固定同步边界是 `RenderRuntime::prepare`。它先调用 `World::sync_for_render()` 收敛 loader 事件和
CPU scene change，再让 `RenderWorld` 消费同步包。render 阶段只读取 prepare 后的 `RenderSceneView`，
不会再回头访问 `AssetHub`、`SceneStore` 或 upload queue。

## 核心对象

`AssetHub` 是一次性 CPU loader service。它分配 `TextureLoadHandle` / `ModelLoadHandle`，
把 texture decode、glTF / Assimp model import 放到后台线程执行，并在 `AssetHub::update()` 中回收完成事件。
它不做 scene identity 分配，也不创建任何 GPU 资源。

`SceneStore` 是 CPU scene 的语义 owner。它保存 scene texture / mesh / material / instance / sky /
analytic light 的运行时身份、展示名称、依赖索引和 `SceneChanges`。Instance name 由导入器或程序化注册调用方提供，
只作为 Editor/debug metadata；opaque `InstanceHandle` 仍是唯一身份。删除 texture、material、mesh 前的引用检查
也在这里完成；失败 edit 不写 change log。

`SceneAssetIngestor` 是 loader 身份到 scene 身份的翻译边界。model 加载完成后，它把 `RawSceneData`
里的 mesh/material/instance index 转成 `MeshHandle`、`MaterialHandle`、`TextureHandle`
和 `InstanceHandle`，并把需要 GPU 上传的 CPU bytes 放入 `SceneAssetSyncOutput`。

`WorldRenderSync` 是 prepare 边界上的同步包，包含两类信息：`SceneChanges` 描述 CPU 语义变化，
`SceneAssetSyncOutput` 携带 texture / mesh 的短期 upload payload。CPU bytes 经过这个包进入 render side，
不会长期留在 `AssetHub` 或 `SceneStore`。

`RenderWorld` 内部的 managers 分别拥有 GPU 派生状态。texture manager 负责 image / view /
bindless SRV，mesh manager 负责 vertex/index buffer 和 BLAS，material manager 负责 stable material slot
和 material buffer，instance manager 负责 stable instance slot、ready gate 和 active render list，
TLAS manager 负责当前 FIF 的 TLAS。

核心状态只由一个明确 owner 持有：

| 状态 | Owner | 边界 |
| --- | --- | --- |
| scene texture / mesh / material / instance / sky / light 语义 | `SceneStore` | CPU 权威状态，不保存 GPU ready 或 slot |
| loader task 与完成事件 | `AssetHub` | 一次性后台任务身份，不作为长期 scene identity |
| loader identity 到 scene identity 的翻译 | `SceneAssetIngestor` | 只在 `World` 内部连接 loader event 与 scene transaction |
| texture image/view/SRV 与 mesh buffer/BLAS | `RenderTextureManager` / `RenderMeshManager` | render-side GPU 派生状态 |
| material/instance stable slot 与 per-FIF buffer | `RenderMaterialManager` / `RenderInstanceManager` | slot 只在 render side 可见 |
| TLAS、sky distribution、analytic/emissive light buffer | 对应 `RenderWorld` manager | render pass 只能通过 `RenderSceneView` 读取 |

owner 边界也决定销毁责任：CPU scene 删除只产生语义 change，实际 GPU 资源释放、迟到 completion 丢弃和
slot 延迟回收都由持有对应资源的 render-side manager 完成。

## 材质类型契约

CPU scene 的 `MaterialData` 是材质语义权威来源。v1 把光学类别和 alpha 覆盖拆成两条正交语义：

- `MaterialClass::Surface`：普通表面；是否 delta / rough 由 shader 根据 roughness 决定。
- `MaterialClass::Transmission { opacity, ior }`：透射表面；`opacity` 只表示透明度，`ior` 表示折射率。
- `MaterialClass::Emissive { radiance }`：显式自发光表面；emissive table 和 closest-hit emission 都从该 class 派生。
- `CoverageMode::Opaque` / `CoverageMode::AlphaMask { alpha_cutoff }`：只决定可见性 alpha test 与 TLAS any-hit。

GPU `PbrMaterial` 不再固定 64B；当前生成 binding 为 80B，并显式写入 `base_color`、`metallic`、
`alpha_factor`、`roughness`、`material_class`、`coverage_mode`、`opacity`、`ior`、`alpha_cutoff`、
`emissive` 与 diffuse/normal bindless handles。`RenderMaterialManager` 只从 CPU
`MaterialClass/CoverageMode` 派生这些字段；closest-hit、any-hit、RayQuery shadow/specular-motion
和 TLAS instance flags 不能各自推断透明或自发光语义。

glTF 导入规则是：`alphaMode = MASK` 只导入为 `CoverageMode::AlphaMask`，未写 cutoff 时使用 `0.5`；
`KHR_materials_transmission.transmissionFactor > 0` 导入为
`Transmission { opacity: 1.0 - transmissionFactor, ior }`，IOR 来自 `KHR_materials_ior`，缺省为 `1.5`；
`emissive_factor` 非零导入为 `Emissive { radiance }`。v1 class 互斥，优先级为
`Emissive > Transmission > Surface`，冲突时打印 warning。`BLEND` v1 不做 alpha blend，coverage 降级为
`Opaque` 并 warning。Assimp/Truvixx 把 `opacity < 0.99` 直接识别为
`Transmission { opacity, ior: 1.5 }`，不再要求低 roughness，也不从标量 opacity 推断 alpha mask。

## 身份转换链路

这个同步机制刻意分离三套身份，避免 loader、CPU scene 和 GPU cache 互相泄漏。

```text
硬盘文件
  -> AssetHub loader handle          // TextureLoadHandle / ModelLoadHandle
  -> SceneStore CPU resource handle  // TextureHandle / MeshHandle / MaterialHandle / InstanceHandle
  -> RenderWorld GPU identity        // bindless SRV / material slot / instance slot / BLAS / TLAS
```

loader handle 只服务后台任务完成事件回收；CPU resource handle 是 App 可编辑、可查询的 CPU 语义身份；
GPU identity 是 prepare 后的派生状态，只在 render-side manager 内部稳定。CPU ready 不代表 GPU ready：
`RawSceneData`、`TextureBytes` 或 `MeshData` 到达后，仍要等 texture / mesh upload 和 BLAS build 完成，
instance 才能通过 ready gate 进入 active render list。

## 一帧中的推进顺序

update 阶段，App 只表达意图：请求 model import，注册 texture / mesh / material / instance，
或修改 material、instance transform、sky、light 等 CPU 语义。此时不会提交 GPU upload。

prepare 开始时，`World::sync_for_render()` 先调用 `AssetHub::update()` drain 后台 loader 事件。
`SceneAssetIngestor` 消费这些事件：texture 成功会形成 `PendingTextureUpload`，model 成功会注册 scene
mesh/material/instance 并形成 `PendingMeshUpload`；随后 `SceneStore::drain_changes()` 输出本帧 CPU 语义变化。

`RenderWorld::prepare_asset_sync()` 先消费同步包中的 asset payload 和删除变化。removed texture / mesh /
material 会先写入对应 manager，确保同一帧的新 upload 或迟到 completion 不会把已经删除的 CPU resource handle 重新发布。
texture 和 mesh upload 通过 timeline 异步完成；完成前 resolver 仍看不到真实资源。

`RenderWorld::prepare_render_data()` 再读取 `SceneReadView`。material manager 从 CPU 材质参数打包
material buffer；instance manager 用 material slot resolver 和 mesh resolver 做 ready gate，并为每个 active
instance 派生 `requires_any_hit`；emissive table、
analytic light buffer、geometry / instance / indirect buffer 和 scene root buffer 在同一 prepare 快照中更新。
TLAS 只基于 active instance 和 ready mesh BLAS 构建或复用：`requires_any_hit == false` 的 instance 会设置
`FORCE_OPAQUE` 跳过 any-hit，含 `CoverageMode::AlphaMask` 的 instance 保留 any-hit。BLAS geometry 不设置 `OPAQUE`，
因为同一 mesh 可能被不同 material 复用。

render 阶段只消费 `RenderSceneView`：shader 从 scene root buffer 读 device address、bindless handle、
light count 和 sky / emissive binding；ray tracing 通过 TLAS custom index 回到 stable instance slot；
raster 通过 prepare 阶段展开的 draw cache 录制 draw。

## 更新、删除与不变量

`SceneChanges` 只表达 CPU scene 语义变化，不表达 GPU ready 状态。material 更新会让 material slot dirty，
并保守地标记依赖该 material 的 instance 重新评估 material binding；这会让 coverage / alpha cutoff 变化
触发 TLAS `FORCE_OPAQUE` 派生重算。instance transform / material binding 更新会影响 instance buffer、
indirect map、emissive table 或 TLAS revision；
sky / light 更新随 scene snapshot 在 prepare 中上传到对应 GPU buffer。

texture 未 ready 或上传失败不会阻塞整个 material / instance。material buffer 会通过 texture resolver 写入 fallback
或 null binding；真实 texture ready 后，只需要重新 dirty material buffer，把 fallback 替换成真实 SRV。

删除先发生在 `SceneStore`。texture 仍被 material 或 sky 引用、material 仍被 instance 引用、mesh 仍被 instance
引用时，删除会失败并保持事务语义。删除成功后，render-side manager 负责移除 ready cache 或延迟回收 stable slot。

已经提交但尚未完成的 texture / mesh upload 不能取消，因此 manager 使用 retired set 处理迟到 completion：
timeline 到达后只销毁资源，不再 publish 到 resolver。material slot 和 instance slot 至少跨过 FIF 窗口后才复用，
避免在飞命令中的旧索引突然指向新对象。

## 失败与 fallback

- texture/model CPU 加载失败由 `AssetHub` event 交给 `SceneAssetIngestor` 收敛到对应 scene/import 状态；
  失败任务不会自动用原 handle 重试。
- model import 在写入 `SceneStore` 前完成依赖校验和 handle 翻译；失败不会提交半套 mesh/material/instance。
- texture GPU 上传尚未完成或失败时，material 继续使用 fallback/null binding，不阻塞整个 instance 进入可渲染状态。
- mesh upload 或 BLAS build 未 ready 时，依赖 instance 保持 pending，不进入 active render list、TLAS 或 raster draw。
- sky texture、distribution build 或 distribution upload 未 ready/失败时，`RenderSkyManager` 使用合法 fallback；
  GPU 派生失败不反向污染 `SceneStore` 的 CPU scene 语义。
- 删除或 revision 变化后的迟到 upload completion 只负责销毁完成资源，不能重新发布 stale handle。
- CPU ready、GPU ready 与 shader-visible ready 是不同状态；UI、日志和调用方不能把任一阶段成功解释为完整链路完成。

最终不变量是：`AssetHub` 不创建 GPU 资源，`SceneStore` 不保存 GPU ready 状态，`RenderWorld` 不反向拥有
CPU scene 语义，render pass 不访问 CPU owner。所有从 CPU 语义到 GPU 可见状态的变化，都必须经过 prepare 边界。
