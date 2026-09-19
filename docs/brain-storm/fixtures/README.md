# Texture mapping 参考资产

[`texture-mapping.gltf`](texture-mapping.gltf) 是自包含 glTF：两张 PNG 与几何 buffer 均内嵌，无个人路径依赖。
18 个 panel 在 XY 平面上按三行六列排列，从 +Z 观察，节点/材质名称携带编号。
UV0 左上为 (0,0)，UV1 为 UV0 的 (1-u, 1-v)。所有 panel 共用两张图片身份，四槽示例的颜色/MR/emissive 共用第一张图。

原始颜色图如下，白色 L 在左上，黑横线在右下，右上有透明圆孔；可用于发现旋转、镜像与 alpha 不一致。

![方向标记](texture-mapping-color.png)

| Panel | 输入/预期检查 |
| --- | --- |
| 00 identity | 与原始颜色图方向相同 |
| 01 offset | offset=(0.25,0.125)，Repeat |
| 02 rotate90 | rotation=π/2、offset=(1,0)；原始 UV (0.25,0.75) 映射为 (0.25,0.25) |
| 03 rotate37 | 非直角旋转，用于发现矩阵转置和符号错误 |
| 04 nonuniform | scale=(2,0.5) |
| 05 negative | scale=(-1,1)、offset=(1,0)，水平镜像 |
| 06 zero | scale=(0,0)、offset=(0.1,0.1)，应为常量采样，无 NaN |
| 07 uv1 | 扩展选择 UV1，颜色相对 identity 绕中心翻转 180° |
| 08 override_uv0 | 普通 textureInfo 为 UV1，扩展覆盖为 UV0，结果与 identity 相同 |
| 09–11 wrap | 相同超出 [0,1] 的坐标，分别 Repeat / Clamp / MirroredRepeat；09 为 Nearest，10–11 为 Linear |
| 12 normal_authored | 有 tangent.w 的旋转 normal 槽 |
| 13 normal_missing | 无 tangent 的旋转 normal 槽，与参考 renderer 的导数 fallback 对照 |
| 14 normal_negative | 无 tangent，normal UV 镜像，检查 handedness |
| 15 normal_zero | 无 tangent，零 scale，回退到未贴图法线 |
| 16 alpha_uv1 | UV1 + 90° rotation 的 alpha MASK；颜色孔洞、阴影和 raycast 应一致 |
| 17 four_slots | 四槽分别使用不同映射；检查 MR 的 G/B、普通表面的 emission、normal scale=0.6 |

本资产用于手动/运行时对照，不是新的单元测试。它提供确定的输入和部分数值预期，不是已验证的参考渲染图。
可通过既有 `GameWorld::import_scene` 导入；当前应用的启动模型由 `TruvisRenderer::request_scene` 指定，
完成后由 Renderer 根据 `SceneData` 显式创建 instance。
CPU 导入、身份复用、编辑校验、shader 执行、画面对照和跨 FIF 更新须分别记录，不能相互替代。

## 当前输入与采样契约

四槽索引为 BaseColor=0、MetallicRoughness=1、Normal=2、Emissive=3。图片身份与槽级映射分离，
颜色/emissive 使用 sRGB image/view，MR/normal 使用 UNORM image/view；来源相同但解释不同的图片
对应独立 `TextureAssetHandle`。本 fixture 有两张来源图片，其中颜色/MR 共源形成两个 handle，
normal 使用第三个 handle。所有材质采样都执行槽级 TRS 后读取 LOD 0。
Filter 仅有 Nearest/Linear，来自 glTF magFilter（未指定则 Linear）；输入 minFilter 不进入内部数据。
不建立 Occlusion 槽，也不附加 AO 路径权重；共享 ORM 图片仍由 MR 引用保留。

数值 oracle 独立从 glTF/PNG 计算：屏幕像素中心投影到 panel，按槽选 UV、执行 TRS，再按 wrap 做 nearest/bilinear。
颜色与 emission 先 sRGB 解码再过滤；MR/normal 保持线性。无光源 Phong 为 `baseColor * 0.5 + emission`，
有点光源时为 `max(Phong, baseColor * 0.5) + emission`，不包含 AO。
法线退化处理与映射边界见 [RT 流程](../../summaries/realtime-rt-raytracing-flow.md)。

## 2026-09-18 四槽实现验收

以下是删除 AO/mip 配置后重新执行的结果。旧五槽验收不能替代本轮证据，不再作为当前能力说明。
相机 `(6,2.4,16)` 朝向 `-Z`，垂直 FOV 60°，原生视口 776×800。DLSS SR/RR 关闭。
临时集成程序、GPU 探针、截图和 oracle 位于忽略的 `build/`，未新增单元测试。
临时启动场景、诊断 shader 与 staging 日志的源码在构建后逐字恢复；正常 shader 与应用已重新构建，交付产物不包含探针。

| 层次 | 本轮证据 | 边界 |
| --- | --- | --- |
| CPU 导入/编辑 | 18 instances、2 张来源图片、四槽、新 Emissive 索引、TRS/UV 覆盖、normal 扩展与强度、非法 UV 原子拒绝、独立 Emissive 编辑和 no-op revision 均通过 | 当时按图片来源共享 handle；不能作为新颜色空间身份契约的验收证据 |
| AO-only 资源 | 增加 AO-only 内嵌不可解码 URI、缺失外部文件及 texCoord=99 的变体仍只发布原有两张图片；不读取/解码 AO 图片，不验证其无关 UV | 不跳过 glTF 结构和 required extension 校验；GLB 共享几何 buffer 仍整体读取 |
| AO 画面对照 | 仅增加 AO 引用/strength/图片的变体与原场景，在相同 PhongPass、相机与点光源下 194,688 个像素逐通道完全一致 | 证明实际 raster 着色不消费 AO；RT/SHARC 的删除同时由共享材质模型与路径状态审查确认 |
| Sampler | 18 个 CPU 编码无重复；GPU 18 组合（3×3 wrap ×2 Filter）共 450 点全部通过，误差不超过 2/255 | 输入 minFilter=9987 被忽略；未验证多级 mip 或抗混叠 |
| RT 法线 | Realtime/Offline 各 16 点，最大误差 1/255 | authored、缺失 tangent、负/零 scale；不代表 MikkTSpace 等价 |
| Raster 四槽 | 现有 PhongPass 接入临时 per-FIF depth target；有点光源时，原场景和 AO 变体各 450 点全部符合独立 oracle，误差不超过 2/255 | 点光源位置 `(6,2.4,10)`、颜色 `(0.4,0.4,0.4)`；覆盖 MR、normal/TBN、emission、alpha、TRS 与 wrap；正常应用编排未改变 |
| Emissive | Offline 25 个采样点中 22 个平坦点符合独立数值预期；另 3 点处于跳变边界，受到累计 jitter 覆盖影响 | 边界点不作为单像素中心 oracle 的通过证据；由 Raster 与 NEE 探针补充 |
| Shadow / NEE | 主光线与生产 RayQuery 遮挡判定在 194,688 像素无差异；Emissive 槽 3 的 NEE/hit 一致。真实 UI 切到 UV1 后 3,156 个内部像素仍一致 | UV1 对照剔除可见鼠标覆盖矩形；这是 GPU 函数一致性，不等于完整 MIS 积分统计验证 |
| Editor / FIF | 真实 Inspector 显示四槽、无 AO、Filter 只有 Nearest/Linear。Emissive offset U 改到 0.25，帧 24014/24015/24016 的 C/A/B staging 全部写槽 3；恢复 0 后帧 25465/25466/25467 的 B/C/A 全部追平 | 未做 device-local material buffer readback；最终 GPU 像素由单独 oracle 验证 |
| SHARC | On(query) 下运行 Update/Resolve；最终正常应用保留原启动场景，query-depth 通道观察到绿色 depth-1 缓存命中。代码确认保留 `SharcSetThroughput(state, material_sample.throughput)`，删除的只是 AO 调制 | 实际 query 命中与缓存画质统计是不同证据，不宣称完成后者 |

ABI 验证：MaterialTexture offsets=0/16/32/36/40/44、stride=48；PbrMaterial textures offset=64、stride=256；
生成 Rust 字段布局与 SPIR-V 一致，四槽数组长度为 4，全局 sampler 数量为 25（7 通用 +18 材质）。
Rust host align 为生成结构的自然布局，验证以字段 offset/数组 stride 为依据，不把 host align 误写成 16。

本地证据：`build/scope-{cpu,check,web,shader,tests}.log`、`scope-hit.spvasm`、
`scope-samplers.png`、`scope-{realtime,offline}-normal.png`、`scope-offline-emission.png`、
`scope-visibility*.png` 和 `scope-normal-stderr.log`。这些是本机验收产物，不作为跨机器 golden image。
Raster 对照为 `scope-raster*.png` 与 `scope-raster-reference.py`；临时 shader 已由 `just shader` 重新生成正常入口。
正常应用的 SHARC 查询截图为 `scope-final-sharc-query.png`，日志为 `scope-final-stderr.log`；启动源码与验收前保存内容逐字一致。
本轮正常应用、Raster、Shadow/NEE 与法线运行日志均未见 Vulkan validation error。
正常关闭时 Vulkan owner 完成释放；WebView2 仍输出既有 `Chrome_WidgetWin_0` 注销错误 1412，
不将其归为材质/Vulkan 失败，也不宣称宿主日志完全无错误。

## 2026-09-18 颜色空间身份复验

按 [颜色空间方案](../material-texture-color-space.md) 实施后，手工 CPU 导入原夹具和忽略 AO 的变体均为
18 个 instance、两张图片来源、三个 texture handle：颜色/Emissive 同源 sRGB，MR 同源 UNORM，Normal
使用另一张 UNORM 图片。此项取代上表按来源共享两个 handle 的旧身份结论；旧 Raster/RT 数值验收
发生在本次颜色空间变更前，不能算作本次的全画面对照。

本次用临时启动分支在 Vulkan 中加载原夹具及已有的 sampler 变体并观察 Realtime BaseColor 通道，
已移除该分支。无遮挡 sampler 变体的 450 个独立数值点中 360 点误差不超过 2/255；前四列
12 个 panel 全部为 25/25。panel 0 临时改为 `[0.5, 0.25, 0.75, 0.6]` 的 BaseColor factor 后，
其 25 个 RGB 点仍全部符合 sRGB 解码后乘 factor 的预期。其余 panel 部分区域出现白色缺口或
图案位置差异，不能把 360 点写作全图通过；目前也不能由此判定差异的具体成因。
Offline、Phong 和同参数 FBX 亮度对照仍需另行采集。截图与临时变体保存在本地忽略目录
`build/color-space-*.png`、`build/color-space-factor.gltf`，不是跨机器的 golden image。


## 构建、残留与 Review 入口

- `cargo check --workspace --all-targets`、`cargo build --bin truvis-app`、`just editor-web` 全部通过；复用 asset/world 既有 6 项测试全部通过。
- `just shader` 重新编译 26 项、缓存 2 项；28 个有效 SPIR-V 通过 `spirv-val --uniform-buffer-standard-layout`。此前删除源码留下的旧 phong.vs 缓存不计入有效入口。
- 源码扫描确认没有 Occlusion 槽、strength、pending AO、空 begin_reflection、TextureMinFilter 或旧材质 DTO 字段。
- 底层 Vulkan min/mag 与固定 mipmapMode 参数仍是有效 API 配置；非材质 Bindless API、image view subresource range、ReSTIR disocclusion 保持原职责，均不是旧材质兼容层。
- 本次采用计划推荐的单 Filter 方案，不保留独立材质 min/mag、旧 Emissive 索引转换、第五空槽或协议迁移器。

Review 调用链：`GltfSceneReader -> RawMaterialData -> AssetSystem/MaterialData -> GpuMaterialStore -> MaterialAccess`；
编辑为 `Inspector -> MaterialPatch -> EditorController -> SceneStore -> per-FIF material upload`。
主要风险分别由 ABI stride/offset、Emissive NEE 与真实编辑、18 sampler 编码与 GPU 对照验证。
保留 CPU World 权威、图片身份、GPU owner、FIF 生命周期、外观 revision、线程与 RenderGraph 顺序。
AO 删除时的路径权重审查同时确认 BSDF/NEE/MIS/Russian roulette 权重与 SHARC 正常 throughput 未被替换或清空。

单级采样不消除缩小/斜视角混叠；本轮不修复来源资产的共面重叠，也不声称覆盖全部 glTF 材质扩展。
未进行 device-local 材质 buffer readback、SHARC 缓存质量统计或全部编辑/回收交错压力验证。
