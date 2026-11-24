# truvis-world

`GameWorld` 持有单个场景的 CPU 状态，包括实例、灯光、环境选择和变更批次。实例保存共享资产的
generational handle。上层使用 Runtime 的 AssetSystem 登记资产并解析有效引用，再调用
`GameWorld::create_mesh_instance`，提供 mesh 的 submesh 数量作为绑定形状约束。
mesh 不变时，改绑继续匹配实例原有的材质数量。

- `game_world.rs` 提供直接的读取和编辑入口。实际实例编辑记录一次身份，删除也记录旧 handle。
  prepare 接管整个批次并读取最终状态。灯光和环境各自拥有 revision。
- `guid_new_type.rs` 定义实例和灯光身份，资源身份由 `truvis-asset` 定义，GPU slot 由渲染 owner 分配。
- `procedural_mesh.rs` 生成可登记的 `MeshData`。每个顶点有一份 UV，零切线由渲染路径按需要重建。
- `GameWorld::update_light` 合并完整 patch 后校验并提交。位置编辑复用这一入口。
  `SceneSkyState` 保存贴图引用、亮度、启用状态和语义版本。

共享资产、加载任务及 GPU scene 由 Runtime 内各自的 owner 管理。update context 负责在删除资产前检查当前引用。相关生命周期见[场景同步设计](../../../../docs/design/scene-sync-and-render-world.md)。
