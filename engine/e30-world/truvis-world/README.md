# truvis-world

`truvis-world` 定义 CPU 侧世界状态，是资源和 scene 数据进入渲染运行时前的聚合点。

## 所有权

- `AssetSystem` 独占 `AssetStore`、`AssetLoadService`、scene import 状态和 texture 去重表。
  它把 `RawSceneData` 转成 `SceneData`，不接收也不保存 `SceneStore`。
- `SceneStore` 独占 `MeshInstanceHandle`、`LightHandle`、sky 状态以及 instance/material/mesh
  反向引用。
- `GameWorld` 组合两个 owner，并在需要同时检查资源引用和场景引用时编排顺序。
- RenderRuntime 只通过 `SceneReadView` 借用最终 CPU 状态；GPU image、buffer、BLAS、descriptor
  和 frame state 属于 render-side owner。
- instance 有效新增、删除及编辑写入去重 handle 集合；Runtime 在 prepare 用 `take_instance_changes`
  接管完整批次并同步对账，不携带字段事件或 GPU ready 状态，不等待 GPU 才清理 CPU 变更。

## 对外流程

```text
GameWorld::import_texture(path, color_space)
  -> canonicalize
  -> AssetSource::File + color space 去重
  -> TextureAssetHandle

GameWorld::import_mesh(mesh_data)
  -> AssetStore
  -> MeshAssetHandle

GameWorld::import_scene(path)
  -> SceneImportHandle
  -> AssetLoadService
  -> AssetSystem::scene_data(handle)
  -> GameWorld::create_mesh_instance(SceneObjectData)
  -> MeshInstanceHandle
```

`procedural_mesh::ProceduralMeshKind` 提供运行时内置几何，包括 `Floor`、`Cube` 和固定
分辨率的 `UvSphere`。这些方法只生成 `MeshData`，不创建 GPU buffer 或 BLAS；场景通过
`GameWorld::import_mesh` 注册后继续使用统一的 RenderWorld 上传路径。

内置几何的三角形绕序与顶点法线一致；Triangle/Rect 位于 XY 平面且正面朝 +Z，Floor
朝 +Y，Cube/UvSphere 朝外。零 tangent 表示沿用上传阶段从 UV 重建的既有约定。

scene import 完成不会自动修改 `SceneStore`。外部可以过滤、复制或重复实例化
`SceneData.objects`。`SceneImportHandle` 的 `LoadStatus::Ready` 只表示 scene 结构和资源身份
已经写入 CPU owner；纹理解码、GPU 上传和最终画面仍是独立阶段。

scene ingest 先校验全部 mesh、材质参数、引用/材质数量与所需 UV，并解析全部外部纹理
路径，再登记资源身份。输入预检失败时不登记任何 mesh/material/texture，也不重试共享
Failed 纹理。embedded 的身份沿用请求入口的 canonical scene path；已拥有的 bytes
不要求源 scene 文件继续存在。场景纹理派发阶段的 Rust unwind 转为该 texture 的 Failed，
完整 scene 结构仍可 Ready，纹理可显式重试；不尝试恢复 abort/OOM。

AssetSystem 持有已接收的完成事件，逐个先提交状态、再输出诊断。诊断 unwind 后，下次
poll 继续消费余下事件；不会把已完成纹理留在 Loading。scene 请求也先成功排队再登记
Loading import；路径错误直接返回可查询的 Failed import，不依赖日志成功。

SceneImportRecord 保留到 AssetSystem 销毁，供状态查询和重复实例化；当前没有单独释放
import record 的 API。SceneData 是导入完成时的描述快照，不替 material/mesh 建立额外
强引用。调用方显式删除其中资源后，旧快照可能含失效 handle，实例化仍由 SceneStore
依赖校验拒绝。批量实例创建和 import record 回收策略不由离线 loader 自动决定。

## AssetSource 与去重

```rust
pub enum AssetSource {
    File { path: PathBuf },
    Embedded { scene_path: PathBuf, image_index: u32 },
    Generated { key: GeneratedSourceKey },
}
```

`TextureRecord` 保存 source、`TextureColorSpace`、CPU 状态和可选 `TextureBytes`。文件路径
进入 `AssetStore` 前 canonicalize；embedded source 使用所属 scene 的 canonical path 和
内部 image index。`AssetSource + TextureColorSpace` 是唯一 texture 去重 key，handle 只表示
资源本身，source metadata 用于诊断、重载和持久化。

显式重复导入 Loading/Ready 纹理会复用原 handle；Ready 内容保持不可变，不读取已修改的
源文件。Failed 纹理可以在文件修复后通过相同导入入口重试，保留原 handle 及 material/sky
引用。重试成功排队后才清除旧错误并进入 Loading；请求未成功返回时不发布新记录，也不
清除已有失败信息。GameWorld 的 file import 在新建或重试排队时推进 scene version，
重复命中 Loading/Ready 不推进。embedded 纹理通过重新导入所属 scene 触发同样的重试。
没有后台自动重试或热重载；Ready 内容变化仍需新身份。

## Instance 编辑

创建和修改 instance 时，world transform 必须有限、affine 且存在有限 inverse；允许 shear
与负缩放，不强制通过编辑器 TRS 分解。失效 handle 或非法矩阵在修改记录、反向引用和
revision 前返回错误。成功的相同值编辑不推进版本，也不新增变更身份。

## 材质编辑

`MaterialData::validate` 在注册和更新前检查普通因子、光学类别、coverage 和 UV transform。
光学类别沿用现有可表示范围：opacity 为 [0,1]，IOR 至少 1，radiance 非负；数值及两路
emission 的和必须有限。alpha cutoff 允许大于 1，只拒绝负值和非有限值。校验不截断输入。
共享材质的 UV 集合需求必须满足每个 live instance 的对应 submesh；失败保持材质、纹理
反向引用和版本不变。相同内容不推进 revision，instance 材质槽重复引用也只计一个引用者。

## 灯光注册与编辑

三类注册入口返回 `Result`，在发布身份前复用编辑约束：位置有限、radiance 有限非负；
spot 方向可归一化且 `0 <= inner <= outer <= pi`；area 半轴需能表示有限、非退化矩形。
area 校验沿用 `AreaLightShape`，不改写合法半轴；spot 保存归一化方向，已归一化值不重复
计算。失败不插入记录或推进版本，注册一次只推进一次 scene/light revision。

`LightTarget::{Point, Spot, Area}(LightHandle)` 显式区分三个独立 SlotMap 的身份，裸 key 不可跨类型查询。
`SceneReadView::light_position` 按类型读取位置；`GameWorld::update_light_position` 只修改 pos/center，
保留方向、角度、半轴和辐射参数。失效 handle 或非有限位置拒绝写入；相同位置不推进版本。
实际变化同时推进 scene version 和 light revision，由既有 AnalyticLightTable 对账并上传 FIF 快照。
World 不保存 gizmo transform、图标尺寸或辅助显示范围。

## 删除与生命周期

`SceneStore` 先检查 live instance/sky 引用，`AssetStore` 检查 material/texture 依赖，
`GameWorld` 再执行 CPU 删除。CPU 删除不等于 GPU 立即销毁；render-side 继续按 FIF 和
completion 延迟回收。mesh/texture 内容不可变，内容变化创建新 handle。

`destroy_scene_mut` 仅服务 Runtime 的终止销毁顺序，随后释放 render-side 并消费 World；
它清空关系与待同步集合，不是运行中的场景清空编辑，不承诺可继续 prepare 的版本通知。

架构约束和导入时序见 [`scene-sync-and-render-world.md`](../../../docs/design/scene-sync-and-render-world.md)。
