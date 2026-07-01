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
analytic light 的运行时身份、依赖索引和 `SceneChanges`。删除 texture、material、mesh 前的引用检查也在这里完成；
失败 edit 不写 change log。

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
material buffer；instance manager 用 material slot resolver 和 mesh resolver 做 ready gate；emissive table、
analytic light buffer、geometry / instance / indirect buffer 和 scene root buffer 在同一 prepare 快照中更新。
TLAS 只基于 active instance 和 ready mesh BLAS 构建或复用。

render 阶段只消费 `RenderSceneView`：shader 从 scene root buffer 读 device address、bindless handle、
light count 和 sky / emissive binding；ray tracing 通过 TLAS custom index 回到 stable instance slot；
raster 通过 prepare 阶段展开的 draw cache 录制 draw。

## 更新、删除与不变量

`SceneChanges` 只表达 CPU scene 语义变化，不表达 GPU ready 状态。material 更新会让 material slot dirty；
instance transform / material binding 更新会影响 instance buffer、indirect map、emissive table 或 TLAS revision；
sky / light 更新随 scene snapshot 在 prepare 中上传到对应 GPU buffer。

texture 未 ready 或上传失败不会阻塞整个 material / instance。material buffer 会通过 texture resolver 写入 fallback
或 null binding；真实 texture ready 后，只需要重新 dirty material buffer，把 fallback 替换成真实 SRV。

删除先发生在 `SceneStore`。texture 仍被 material 或 sky 引用、material 仍被 instance 引用、mesh 仍被 instance
引用时，删除会失败并保持事务语义。删除成功后，render-side manager 负责移除 ready cache 或延迟回收 stable slot。

已经提交但尚未完成的 texture / mesh upload 不能取消，因此 manager 使用 retired set 处理迟到 completion：
timeline 到达后只销毁资源，不再 publish 到 resolver。material slot 和 instance slot 至少跨过 FIF 窗口后才复用，
避免在飞命令中的旧索引突然指向新对象。

最终不变量是：`AssetHub` 不创建 GPU 资源，`SceneStore` 不保存 GPU ready 状态，`RenderWorld` 不反向拥有
CPU scene 语义，render pass 不访问 CPU owner。所有从 CPU 语义到 GPU 可见状态的变化，都必须经过 prepare 边界。
