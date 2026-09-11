# CPU AssetSystem / GameWorld 到 GPU RenderWorld 同步

> 状态：当前实现事实总结（2026-09-10）。主设计见 [`render-scene-mirror-and-resource-system.md`](../brain-storm/render-scene-mirror-and-resource-system.md)。

## 机制定位

Truvis 现在把 CPU 资源、CPU 场景关系和 device 资源分成三个边界：

```text
AssetHub / loader
      │ completion 写回 CPU registry
      ▼
GameWorld
├── AssetSystem   mesh / material / texture 身份、内容与依赖
└── SceneStore       instance / light / sky 关系与反向索引
      │ SceneReadView
      ▼
RenderRuntime
├── RenderAssetSystem   texture / mesh / material / sky 的 GPU 资源与上传
└── RenderWorld            instance 镜像、GPU scene buffer、emissive、TLAS、历史
      │ RenderSceneView
      ▼
render pass / shader / raycast
```

`GameWorld` 是 update 阶段唯一的 CPU facade。它不保存 Vulkan 对象；`AssetSystem` 也不创建 GPU
对象。`RenderAssetSystem` 是 runtime 级共享 GPU 资源 owner，`RenderWorld` 只组合一个场景的
instance、light、sky 镜像和派生 GPU scene。prepare 完成后，render pass 不回读 CPU owner。

## CPU owner

`AssetSystem` 位于 `engine/e30-world/truvis-world/src/asset_system.rs`，包含
`AssetStore`、`AssetHub` 和 `SceneAssetIngestor`：

- mesh 注册时由 CPU `AssetStore` 保留不可变顶点/index payload；渲染侧只在需要上传时借用它。
- texture 注册后获得 `TextureHandle`，file 或 embedded bytes decode 由 `AssetHub` 异步完成；外部路径按 canonical path、embedded image 按 scene path + image identity 复用同一个 live handle。
- material 保存完整 `MaterialData` 和 source revision，更新内容时维护 `material -> texture` 依赖。
- mesh/texture 创建后不可变；不同内容使用新 handle。material 可以更新，但相等赋值不推进 revision。
- 删除 material、mesh、texture 前分别检查 live instance、material 和 sky 引用；失败不修改表和版本。

`SceneStore` 只保存 instance、analytic light、sky 和 `material -> instance`、`mesh -> instance` 反向索引。
创建或修改 instance 时，通过 `AssetStore` 校验 mesh/material handle 和 material 数量；不支持修改
instance 的 mesh 内容或 mesh 引用。`SceneReadView` 同时只读借用 SceneStore 与 AssetStore，向
render-side 暴露最终状态、资源 membership、material revision 和引用查询。

`SceneAssetIngestor` 负责把 loader handle 翻译成 CPU handle。model 完成后先校验 raw scene，随后
注册资源和 instance；结构导入完成即表示 model import `Ready`，material 可以引用仍处于
`Loading` 的 texture。外部路径和 embedded bytes 都通过同一个 texture completion event 写回
`AssetStore`。首期不做导入事务回滚，半途失败留下的无引用资源可以通过普通删除接口清理；单张
texture 失败保留 scene structure 并由 render-side 使用 fallback。

## GPU owner

`RenderAssetSystem` 位于 `engine/e40-render/truvis-render-runtime/src/render_world/render_asset_system.rs`，
由 `RenderRuntime` 持有，包含：

- `GpuTextureStore`：按 texture handle 安装 image/view/SRV，维护 fallback、published binding revision 和迟到完成回收。
- `GpuMeshStore`：按 mesh handle 创建 vertex/index buffer、RtGeometry 和 BLAS；mesh 内容不可替换，删除后跨 FIF 延迟释放。
- `GpuMaterialStore`：扫描 CPU material membership，维护 stable slot、render-side source snapshot、texture binding revision 和每 FIF dirty upload。
- `GpuSkyStore` 与 `GpuAssetUploadQueue`：处理 sky fallback/distribution 以及 texture/sky 的异步完成。

这些 manager 不保存 instance 的组合关系；material slot、image 和 BLAS 可被同一个 runtime 的场景引用。`RenderWorld`
中的 `RenderInstanceTable` 只保存 CPU instance handle 到 stable instance slot 的镜像、transform/material
快照、pending/active 状态和 motion history；`SceneTlas`、emissive table、scene/indirect buffer
和 raster draw cache 都属于场景侧。

## 一帧同步顺序

`RenderRuntime::prepare` 的固定顺序是：

1. `GameWorld::poll_asset_loads()` 调用 `AssetHub::update()`，让 `AssetSystem` 消费 loader 完成事件并写回 CPU resource/scene 最终状态。
2. `RenderAssetSystem::sync()` 对账 CPU membership，移除已删除资源，借用保留的 texture/mesh 内容提交上传，poll/publish GPU completion，再扫描 material source/binding revision。
3. `ShaderBindingSystem::prepare_render_data()` 刷新全局 bindless 表。
4. `RenderWorld::prepare_render_data()` 扫描完整 instance membership，比较 transform、material 列表和 material revision，解析 mesh/material ready gate，生成 `RenderData`。
5. analytic light、emissive table、geometry/instance/indirect buffer、TLAS 和 scene root buffer 依据本次对账结果更新；最后写 per-frame 数据。
6. render graph 只通过 `RenderSceneView` 使用当前 FIF 的 scene root、TLAS 和 draw cache。

完整扫描包含隐藏、pending 和未 ready instance；不使用视锥可见列表作为删除依据。扫描中未出现的 handle
才会退役 render-side mirror。generational handle 防止 slot index 复用时把新对象当成旧对象。

## 变化发现与 dirty 的剩余边界

生产主路径不再有 `SceneChangeLog`、`DirtyEvent`、`DirtyRule` 或 `DirtyDispatchPlan`。CPU edit 只写最终
状态并推进对象 revision；接收方在 prepare 比较最终状态。这样同一帧的移动+换材质、A→B→C 连续编辑和
同步前创建又删除都不需要抵消事件。

“取消 changelog”不等于删除所有 dirty：

- material slot 的 per-FIF dirty 是 GPU 副本尚未追平的 bookkeeping，不是跨层语义事件。
- texture binding revision 用于发现 CPU 未编辑但异步 texture 已 ready；mesh/BLAS completion 也独立推进。
- emissive、TLAS 和 analytic light 只保存本地待上传/重建状态，由完整对账结果触发。
- `RenderData` 是本次 prepare 的只读打包视图，不是第二份 CPU 权威。

版本含义彼此独立：CPU handle generation、source revision、published resource/binding revision、
per-FIF uploaded revision、TLAS/emissive/appearance 版本不能合并成一个全局 dirty 数字。

## 删除、迟到完成与 FIF

CPU 删除先检查反向索引，再从 AssetSystem 或 SceneStore 移除。render-side 下一次完整扫描发现
membership 缺失后退役 slot/cache；已提交但尚未完成的 texture upload 或 BLAS completion 只销毁结果，
不能重新 publish stale handle。mesh buffer、image、material slot 和 instance slot 按各自 owner 的约束
延迟回收，不能把 CPU 删除直接等同于 GPU 已安全释放。

`RenderRuntime::begin_frame` 先等待当前 FIF timeline、清理底层延迟释放，再向
`RenderAssetSystem` 和 `RenderWorld` 传递 frame id。每个 material buffer、scene buffer 和 instance
buffer 有自己的 FIF 副本；本帧只写当前 label，落后的 A/B/C 副本会在再次使用前补写最新目标。CPU
loader 完成、GPU copy 入队、timeline completion、shader-visible publish 和 render 结果是不同阶段，日志
中的任一成功都不能替代后续阶段证据。

## 支持范围

首期实现和 CPU 单元测试覆盖：

- FBX/glTF model import 以及程序化 mesh/material/texture 注册。
- instance 创建、transform 修改、material 引用列表修改、material 内容修改。
- material、mesh、texture 的反向引用查询和孤儿删除；删除被引用资源会被拒绝。
- mesh/texture 不支持内容修改或同 handle 热替换。
- CPU scene/resource 变化、异步 texture/mesh ready 和 GPU 资源退役在 FIF 下保持有效。

不在首期范围：导入事务回滚、多 GameWorld 资源协调、跨线程 RenderWorld、完整 ECS extraction、mesh/texture
热替换和按属性的复杂事件图。

## 代码入口

- CPU：[`asset_system.rs`](../../engine/e30-world/truvis-world/src/asset_system.rs)、[`scene_store.rs`](../../engine/e30-world/truvis-world/src/scene_store.rs)、[`scene_asset_ingestor.rs`](../../engine/e30-world/truvis-world/src/scene_asset_ingestor.rs)
- GPU：[`render_asset_system.rs`](../../engine/e40-render/truvis-render-runtime/src/render_world/render_asset_system.rs)、[`render_world.rs`](../../engine/e40-render/truvis-render-runtime/src/render_world/render_world.rs)
- Cornell 验证：[`cornell_renderer.rs`](../../renderer/samples/cornell/src/cornell_renderer.rs)

`cargo test -p truvis-world`（4 项）、`cargo check -p truvis-render-runtime -p cornell-renderer -p cornell-app` 和
`just cornell` 已通过构建并启动到渲染循环；启动日志观察到 FBX 导入 11 个 runtime instance、sky texture 发布和 sky
distribution 发布。该启动检查证明资源发布主路径可运行，但没有覆盖编辑器连续修改、删除或长时间 FIF 压力，也不等价于逐像素图像回归。
