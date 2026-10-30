# truvis-asset

资产加载模块只负责一次性的 CPU 文件读取、纹理解码和 scene 导入。它位于 `AssetSystem`
和 RenderRuntime 之间，不创建 Vulkan image、buffer、BLAS、descriptor 或 material slot。

## 组件与边界

- `AssetLoadService` 分配 `TextureLoadHandle` / `SceneLoadHandle`，保存一次性请求描述，
  收集 worker 结果并发布 `AssetLoadEvent`。
- `AssetLoadWorker` 隐藏 Rayon 线程池和结果 channel。后台任务只读取文件、解码纹理、
  导入 scene 并复制 owned Rust 数据，不修改 `AssetStore`、`GameWorld` 或 `SceneStore`。
- `TextureLoadDesc` / `SceneLoadDesc` 是 task 输入，不承担长期资源身份；`TextureBytes`、
  `MeshData` 和 `RawSceneData` 是 CPU completion payload。
- `AssetSystem` 消费事件并分配 `TextureAssetHandle`、`MeshAssetHandle`、
  `MaterialAssetHandle`；scene 完成后发布 `SceneData`，外部再创建 `MeshInstanceHandle`。

`TextureLoadHandle` 和 `SceneLoadHandle` 只在加载阶段存在。它们不表示长期资源、GPU slot
或 live runtime instance。CPU `Ready` 也不表示 GPU upload 或 descriptor publish 已完成。

service 先完成请求诊断，再登记短期 handle 并派发；只从 worker channel 收集结果。
完成时移除请求记录并交付一次事件；服务内不额外缓存事件或去重，长期身份由 AssetSystem 管理。

worker 在完整 task 入口捕获可展开的 Rust panic（包括任务诊断），转为对应失败结果；
不处理进程 abort 或 native 崩溃。销毁时等待已排队任务退出，等待路径不调用 logger。
这不是取消或发布完成：没有轮询的结果随服务丢弃，阻塞 IO 仍可能延长退出等待。

## Scene 与纹理输入

glTF Reader 接受 major 2 的文档；`version`/`minVersion` 必须是十进制 major.minor。
声明 `minVersion` 时仅接受当前支持的 2.0，且不能高于 version；没有更高最低要求时，
2.x minor 更新不会仅因版本号被拒绝。JSON 与 GLB 的 JSON chunk 使用同一检查，先于资源 IO。

`extensionsRequired` 必须包含于 `extensionsUsed`，两个列表均不允许重复项。必需扩展
目前只允许已有导入入口的 KHR_texture_transform、KHR_materials_transmission、
KHR_materials_ior、KHR_materials_emissive_strength。KHR_lights_punctual 即使库可解析，
也因 Reader 没有灯光载荷而拒绝；optional 未实现扩展仍可忽略。
名单只证明存在转录入口，不证明所有扩展字段或 GPU 语义完整支持：当前 transmission
只转录 factor，IOR 只保留在透射材质，完整材质支持仍受现有渲染契约限制。
不支持的语义按对象/特征记录警告并降级：BLEND 使用 Opaque，顶点色、动画、morph 和
skin deformation 不参与输出；transmission texture 忽略、保留 factor，非透射 IOR 忽略。
结构错误、非法引用/长度和非有限数据仍失败，包括被忽略语义引用的 accessor。

材质数值在 glTF 转录前按输入域校验：base color、emissive factor、metallic、roughness、
transmission 位于 [0,1]；emissive strength 有限且非负，允许 HDR；normal scale 只要求有限，
不禁止负值。IOR 必须有限且为 0 或不小于 1；透射材质的 IOR=0 特殊模式当前无法表达，明确失败。
MASK cutoff 必须有限且非负，保留大于 1 的值以完全遮罩；其它 alpha 模式忽略 cutoff。
该入口不修改通用 CoverageMode 构造器或 World 编辑 API 的契约。

当前纹理 sampler 仅保存双轴 wrap 与一个 filter：导入使用 magFilter（缺省 Linear），
GPU 的 min/mag 共用该值，固定 LOD 0，未独立支持 minFilter/mipmap。doubleSided 和
occlusion 也尚未进入 raw 材质；这些是已知跨层缺口，不能视为完整 glTF 材质支持。

node 声明在资源 IO 前校验为无环、单父森林；同一 scene 的 root 不得重复或指向 child，
同一 root 可用于不同 scenes。选择默认 scene，未指定时取第一项；无 scene 的资源库暂不接受。
实例复制使用显式栈，保持原深度优先顺序与 `parent * local` 累积，不以调用栈限制层级深度。

选中 node 的 matrix/TRS 不得同时声明；rotation 必须为单位 quaternion，局部 matrix
必须有限、仿射且无 shear。累计 world matrix 可含 shear；实际发布的 mesh instance 必须
能由当前 f32 求逆路径得到有限逆矩阵。glTF 本身允许零缩放，但本项目不发布奇异 mesh 实例。
空 group 的零缩放仍可存在，未被选中 scene 访问的变换不在此入口求值。

Reader 使用现有 node transform，不播放动画；skin 降级为“未蒙皮基础网格 + 现有 node
transform”，不承诺 bind pose。morph 不求值，但 target 数量、权重长度与有限性仍须有效。

Assimp / glTF loader 只复制 owned scene 数据。glTF loader 在把外部 image URI 复制为
`RawTextureSource::ExternalPath` 前完成 percent-decoding，并对非法 percent escape 或无效
UTF-8 直接失败；`RawTextureSource` 随后保留已解码的本地路径或 embedded bytes。路径解析、canonicalize、`AssetSource` 构造以及
`AssetSource + TextureColorSpace` 去重由 `truvis-world` 的 `AssetSystem` 负责。
Embedded source 使用 canonical scene path 与 image index 表示。

Assimp Reader 在返回成功前复用 `SubmeshData::normalize_imported_frame` 归一化/修复法线并检查几何，并校验实例的
mesh/material 引用范围。实际发布的 mesh instance 必须具有有限仿射 world matrix，且当前
f32 求逆结果有限；负缩放和 shear 仍允许，无 mesh 的 group 不发布、不单独求逆。
此检查只保证已复制载荷的局部前置，不代替 native 指针/长度契约，也不保证完整场景登记的
事务性或所有 Assimp 格式、材质、动画语义已受支持。

Assimp 标量转录优先使用 PBR base color，缺省时使用 diffuse；metallic 只来自 metallic
factor，不将 reflectivity 当作金属度。Assimp 5.4.3 的 FBXConverter::SetShadingPropertiesCommon
已将 EmissiveFactor 乘入颜色；本层不再乘该因子。独立 Maya|emissive_intensity 映射到
AI_MATKEY_EMISSIVE_INTENSITY，converter 未应用，native 侧保留一次 RGB 乘法。Rust
将结果放入独立 emissive factor，不改变表面的散射类别。输入 base color 必须有限，
emission 有限非负，metallic/roughness/opacity 位于 [0,1]；失败先于类别构造器的 clamp。
现有 opacity < 0.99 到 Transmission、默认 IOR=1.5、coverage=Opaque 的映射仍是项目近似，
不等于按外部格式的透明混合、体积吸收或 specular/glossiness 模型完整转换。

Assimp 颜色贴图优先 BASE_COLOR，缺省时使用 DIFFUSE；normal 使用 NORMALS。所消费槽
消费首层外部路径；不支持的映射、transform、wrap、blend、op 与 flags 按槽警告，使用
UV0、identity、repeat、multiply/blend=1 和无 flags。非有限参数仍失败。embedded key（包括具名内嵌图片）不能被当作
外部文件名。当前路径型 C ABI 仍不承载其它贴图类型、多 UV 或动画，不能声称完整格式支持。
Assimp native 对 skin/morph/顶点色和动画警告后忽略，保留未蒙皮基础网格及现有 node
transform，不承诺 bind pose。拒绝不完整/退化三角几何、非法引用与非有限数据。
UV0 只取前两个分量，缺失分量补零；静态节点矩阵按层级累计，不复制相机/灯光。额外未消费 UV 集忽略；native 的 XYZ tangent 不含 handedness，Rust 继续使用零切线
标记交给现有 shader 重建，不宣称与烘焙切线等价。

glTF buffer 先检查真实字节长度，再限制到声明的 `byteLength`；bufferView 必须落在
声明范围内。文件额外字节和 GLB padding 可以存在，但不能补足短输入或作为越界 view 的数据。

glTF 在 typed reader 前检查全部 accessor 的范围和浮点有限性，并检查 primitive 属性与 indices
的类型、数量、步长、对齐和字节范围。sparse 覆盖必须有合法范围及严格递增、未越界的索引；
支持有底层数据和零初始化两种 sparse base。声明损坏的属性必须失败，只有确实缺失的
indices/normal/tangent 才沿用默认生成；索引不得使用原 component type 的 restart 值。
输出仍由 `SubmeshData::validate` 检查长度、有限数值和三角索引范围；不会重复建立 mesh 校验层。

缺失 NORMAL 时按三角形角点展开 positions 和所有 UV，生成 flat normals 并丢弃输入 tangent。
三角形顺序和绕序保持；逐面方向用 f64 计算，精确零面积面作为退化拓扑拒绝。
展开前后均检查 CPU mesh 契约和 u32 寻址容量；已有 NORMAL 时保留原顶点/索引布局。
零切线仍由现有 shader 根据 normal UV 差分重建；这不是 MikkTSpace 烘焙切线的等价保证。
有限非零 NORMAL 使用 f64 归一化，零法线取首个相邻有效几何面的方向；没有相邻面的零法线
仍拒绝。非零 authored TANGENT.xyz 保留单位方向约束；有限非零 w 归一为 ±1，零 w 或零
方向沿用 shader 重建标记。修复警告按 submesh 汇总。这不是完整 glTF 几何合规检查。
count 的 u32 上限保护寻址和 sparse 游标，不是内存预算；大输入仍可能耗尽 CPU 内存。

buffer/image 共用严格 URI 解码：data URI 先解码 percent escape，再按末尾 base64 标志
解码一次；普通未转义 base64 借用原文本。外部路径拒绝空值、NUL、远程 scheme 和未编码
的 query/fragment；支持本地相对/绝对路径，`file:` 前缀后须为平台可识别的绝对路径，
不处理 URL authority。路径定位与文件存在性仍由对应 buffer 读取或 AssetSystem 负责。

图片解码不翻转行；glTF UV 保持原值，Assimp 继续由 C++ 导入阶段处理坐标约定。
`TextureBytes` 校验 extent 和四通道元素数量，并根据 payload 推导 Vulkan format；HDR/EXR
使用线性 RGBA16F，普通图片按显式 `Srgb`/`Linear` 解释为 RGBA8。

文件输入按扩展名选择 decoder，embedded bytes 按内容识别，`mime_type` 仅用于诊断。
普通图片包括 16 位整数图片，转为 RGBA8 时会量化；不保留原始整数精度。
浮点图片只接受 Linear：loader 将负/非有限 RGB 清零、有限值限制到 binary16 范围；
alpha 限制到 [0,1]，非有限 alpha 取 1。公开 `TextureBytes::new` 只校验尺寸与数量，
直接构造 F16 payload 的调用方仍负责数值语义。CPU 校验不证明 GPU 尺寸支持或分配成功。

## 相关入口

- `asset_load_service.rs`：服务和 completion event。
- `asset_load_worker.rs`：后台任务调度。
- `handle.rs`：load desc、load handle、CPU payload 和 raw scene 类型。
- `gltf_scene_loader.rs` / `truvixx_scene_loader.rs`：格式导入任务。
- `texture_loader.rs`：图片解码任务。
- 根目录 [`scripts/scene/export_gltf.py`](../../../scripts/scene/export_gltf.py)：离线导出 glTF 及外部贴图；
  相机和 glTF 可表达的灯光随场景导出，不生成独立的 manifest 或 metadata JSON。

长期资源 source、去重和 scene import 状态见 [`truvis-world`](../truvis-world/README.md) 与
[`scene-sync-and-render-world.md`](../../../docs/design/scene-sync-and-render-world.md)。
