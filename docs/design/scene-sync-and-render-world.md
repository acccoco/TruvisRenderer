# Scene Sync 与 RenderWorld

> 类型：设计文档。本文定义 CPU scene、资源身份和 GPU scene mirror 之间的同步边界。

## 权威状态

Runtime 持有一组 World 和一个设备域内共享的 `AssetSystem`。`GameWorld` 保存实例、灯光和环境的权威状态，
`RenderWorld` 直接借用 `&GameWorld` 与 `&AssetSystem`，产出同一个场景的 GPU 表示。资源生命周期独立于 World。
`GpuSkyStore` 随 World 保存天空采样分布，HDRI 纹理属于共享资产。

`AssetLoadService` 在后台读取和解码，返回内存数据。Runtime 的 `AssetImports` 在 update 边界领取完成，
按任务身份登记资产或发布导入描述；上层选择何时把描述实例化到 World。导入预检先验证 mesh、引用、
材质参数、可用 slot 和外部纹理路径，再提交资源。失败的纹理可以由调用方显式以原 handle 重试。

## 身份、材质表和就绪

- `AssetSystem` 持有 `MaterialTable`。材质注册成功同时交付 generational handle 与稳定 GPU slot。
  当前帧副本在 prepare 上传，之后才可提交使用该行的 pass。
- `RenderWorld` 持有每个 FIF 的 `MaterialSlotTable`。它为实例的 submesh 保存共享材质 slot，
  shader 由 instance 和 submesh 经该表定位 `MaterialTable`。
- 合法场景引用由上层提交。实例等待 mesh 的几何 buffer 和 BLAS 发布，随后进入 GPUScene、
  TLAS、draw cache 和拾取快照。材质 slot 在实例提交前已经存在。
- 未发布的颜色贴图使用诊断纹理，其它通道使用 null binding。AssetSystem 在 prepare 对账纹理发布，
  纹理内容变化更新原材质行，保持 MaterialSlotTable 的索引。

## 最终状态对账

GameWorld 的变更批次只保存去重后的 generational instance handle，包括删除的旧 handle。
RenderWorld 接管整个批次并查询最终状态，初次建立时读取完整 membership。
后续主动查询等待实例的 mesh readiness，以及实际可见材质的外观和发光版本。
coverage 的最终 any-hit 要求由活跃实例的材质参数计算。资源 owner 的发布和场景准备各自在其 owner 内确认。

已产生的实例变更保留到快照交付。上传、TLAS、发光表和历史的失效状态一旦观察到就写入对应 owner，
FIF 副本和语义版本分别确认。MaterialSlotTable 在实例成员、绑定或布局变化时重写。

## Prepare 顺序

1. update 应用加载完成和上层编辑，GameWorld 收集变更身份。
2. Runtime 推进共享 transfer queue 和 mesh graphics timeline，纹理发布由 AssetSystem 接收，
   天空分布完成由 RenderWorld 接收。
3. Runtime 更新 bindless descriptor，录制当前 FIF 的 MaterialTable dirty rows。
4. RenderWorld 查询 GameWorld 和 AssetSystem，准备场景各表、TLAS、draw cache 和 scene root。
5. graphics submit 等待所发布 transfer 资源的 timeline，提交成功确认对应 FIF。RenderGraph 和同步拾取读取准备后的场景。

## 删除边界

update context 对当前 World 的实例、材质和天空引用执行检查。
晚到的异步完成通过 generational handle 校验，仅释放失效任务的结果。

## FIF 与 capacity

每个 material buffer、scene buffer 和 instance buffer 维护 per-FIF 副本。本帧只写当前 label；落后副本在再次使用前补写最新目标。

`RenderInstanceTable` 只在最终 transform 改变时推进实例版本。dirty 集合保存尚需运动历史或 FIF
同步的 Active 身份；各副本记录上传的 current/previous 版本对。常规上传仅写不匹配的
`model/inv_model` 或 `prev_model`，不比较 staging 矩阵，不复制完整 instance 表。
结构 revision 仅控制集合、绑定布局与容量变化的完整上传；transform 仍更新 TLAS、emissive 和累计签名。

上传 submit 返回只确认 GPU 副本版本；主视图 render submit 返回才推进上一渲染帧 transform。
矩阵准备结果先由调用者持有，copy/barrier 完整录制后才移交 table；提交确认必须匹配
同一 FIF 和冻结的身份/版本。历史与副本确认均先整批校验，不允许后项违约留下部分推进。
已录制但未确认时拒绝再次 prepare，不能通过丢弃记录绕过 command/提交结果的处置。
相机历史使用同一提交边界。无主视图提交保留历史；present 失败不回滚已提交历史。
物体停止后继续保持 dirty，直到全部 FIF 都为 `(current, current)` 且渲染历史已追平。
新增/重新激活令 previous=current；buffer 重建只重写当前副本，保留 CPU 运动历史。

动态容量扩展必须同时更新 CPU mirror、GPU buffer、descriptor count 和当前 FIF 的上传路径。扩展不能改变已有 handle 语义，也不能在 GPU 仍使用旧 buffer 时直接销毁旧资源。

SceneBuffers 的 device/stage 成对交付；当前 FIF 安全点之后才替换旧 pair。替换产生的
完整上传需求必须保留到提交确认，不能只依赖一次 ensure_capacity 的临时返回值或 CPU
scene revision。后续准备中断时新 pair 仍有 owner，成功确认只清除当前 FIF 的需求。

## 实现入口

- [GameWorld](../../engine/runtime/e30-world/truvis-world/src/game_world.rs)
- [AssetSystem](../../engine/runtime/e40-render/truvis-render-runtime/src/asset_system.rs)
- [MaterialTable](../../engine/runtime/e40-render/truvis-render-runtime/src/asset_system/material_table.rs)
- [RenderWorld](../../engine/runtime/e40-render/truvis-render-runtime/src/render_world/render_world.rs)
- [RenderInstanceTable](../../engine/runtime/e40-render/truvis-render-runtime/src/render_world/render_instance_table.rs)

`RenderSceneView` 只暴露当前 FIF 可安全使用的 scene root、TLAS 和 draw 数据。源资产、实例编辑和
GPU 同步只发生在各自的 Runtime phase。

## 版本语义

以下版本各自表达不同事件：

- handle generation：CPU slot identity 是否仍有效；
- source revision：CPU asset/material/instance 内容变化；
- published revision：GPU manager 已发布的 shader-visible 版本；
- per-FIF uploaded revision：当前 frame label 副本是否追平；
- TLAS/appearance version：局部 GPU 结构是否需要重建。

把它们压成单一 dirty counter 会丢失异步和 FIF 语义，因此同步代码应保留各自 owner。

## 变更检查

- 新资源是否先进入 CPU owner 再进入 RenderWorld？
- GPU slot 是否能从 CPU handle 安全反查？
- 实例是否在 mesh 和该帧材质表有效之后提交？
- 删除是否记录完整 generational handle 并在局部对账中退役？
- late completion 是否会重新 publish stale handle？
- 新 buffer capacity 是否同步 descriptor 和每个 FIF？

## 非目标

本文不定义完整 glTF/FBX importer 格式，不描述具体 RenderGraph pass，也不把当前支持范围写成未来能力承诺。

资源导入、CPU scene mutation 和 GPU upload 分属不同阶段；需要解释某个协议或 UI 恢复行为时链接 Editor design，而不在此复制。

## 灯光与环境参数编辑

`GameWorld::update_light` 按 patch 合并当前三张灯光表；位置 Gizmo 复用该入口。
所有候选值校验后一次写入，失败和 no-op 不推进 light/scene revision。
Area 以 center/half_u/half_v 为权威，旋转和尺寸不另存缓存。

`SceneSkyState` 拥有 enabled、texture、brightness 和语义 revision；有效倍率为 enabled ? brightness : 0。
prepare 将倍率和 Sky revision 写入当前 FIF 的 CPU 渲染投影，通过 RenderSceneView 只读提供给 pass，
仍使用已有 RT push constant，不增加 shader ABI。关闭环境保留纹理资源，亮度改变不重建 importance distribution。
Sky 语义 revision 与异步 distribution 发布版本独立，均参与各自消费者的历史判定。
