# truvis-asset

资产加载模块，提供 texture / model 的一次性 CPU loader task 和完成事件。

本模块位于 GameWorld 层和 RenderRuntime 之间，只负责到 upload-ready CPU data：
不创建 GPU image / image view，不创建 vertex/index buffer、BLAS，也不注册
bindless descriptor 或 material slot。GPU 上传和 shader 可见绑定由
`truvis-render-runtime` 的 `GpuTextureStore`、`GpuMeshStore`、
`GpuMaterialStore` 负责；model import 由 `GameWorld` 内部的
`SceneAssetIngestor` 在 asset sync 阶段实例化成 runtime instance。Renderer 层不直接持有
`ModelLoadHandle`，而是通过 `GameWorld` 的 `ModelImportHandle` facade 查询 model import。

## 主要组件

- `AssetHub`：对外统一入口，负责 loader handle 分配和完成事件汇聚
- `AssetLoadEvent`：CPU 数据完成事件，交给 `SceneAssetIngestor` 翻译成 CPU resource handle 和 render upload event
- `TextureLoadDesc` / `ModelLoadDesc`：一次性 loader task 输入描述，不承担长期去重 identity；texture request 携带显式 `TextureColorSpace`，支持外部文件和带 MIME 的 embedded bytes，`AssetHub::request_texture_bytes` 是内存请求入口
- `TexturePixels` / `TextureBytes`：从图片文件解码出的共享 owned CPU payload；普通图片为
  带 `Srgb`/`Linear` 解释的 `Arc<[u8]>` RGBA8，HDR/EXR 为 `Arc<[u16]>` RGBA16F bit pattern；
  只通过事件交给 render-side owner，format 由 payload 推导
- `SubmeshData`：从导入器复制出来的 owned CPU 几何数据，是 scene / GPU scene / RT 中的最小几何单元
- `MeshData`：由一个或多个 `SubmeshData` 组成的 owned CPU mesh payload，只通过事件交给 mesh manager；mesh 对应一个 BLAS，submesh 对应 BLAS 内一条 geometry
- `RawSceneData`：model 导入后的 owned CPU scene payload，通过 `ModelLoaded` 事件交给 `SceneAssetIngestor`；material texture 以 `RawTextureSource` 表达外部路径或带 MIME 的 embedded bytes

## 内部结构

- `asset_loader`：crate 内部后台调度层，只持有 Rayon 线程池、结果 channel 和任务等待逻辑。
- `texture_loader`：crate 内部纹理任务实现，只负责 image 文件读取、CPU 解码和 RGBA8/RGBA16F payload 输出。
- `truvixx_scene_loader`：crate 内部 Assimp scene 导入任务实现，通过 `truvis-assimp-binding` 调用 Assimp C API，只负责 C++ importer 生命周期和 owned CPU scene 数据复制；当前每份导入几何显式包装为单 submesh mesh。
- `gltf_scene_loader`：crate 内部 glTF / GLB scene 导入任务实现，通过 Rust `gltf` crate 读取 material / mesh / instance，并复制成与 Assimp 路径相同的 owned CPU scene 数据；当前每个 glTF primitive 仍包装为单 submesh mesh。
- 外部调用方不直接使用 loader 模块；加载请求、状态查询和完成事件都通过 `AssetHub` 进入或离开 asset 层。

## 设计目标

- 避免阻塞渲染主流程
- 通过状态机管理 Loading -> Ready / Failed 过程，其中 Ready 只表示 CPU ready
- `TextureLoadHandle` 表示 texture load task 身份，不表示长期 scene texture 或 GPU binding
- `ModelLoadHandle` 表示 model load task 身份，不表示长期 prefab 或 live runtime instance
- Assimp 导入任务只在后台复制 owned CPU 数据，完成后释放 C++ scene handle，不把 C++ handle/raw pointer 传出任务
- glTF 导入任务只在后台复制 owned CPU 数据；`.gltf` / `.glb` 由 asset loader 按扩展名分派，其它格式继续走 Assimp 路径
- Assimp / glTF 导入失败会通过 `ModelFailed` 事件回传给 `SceneAssetIngestor`
- model material 引用的相对纹理路径按 model 文件所在目录解析，绝对路径保持不变；asset 层不做 scene texture identity 去重或 canonicalize，后续由 `GameWorld` / `SceneAssetIngestor` 的 scene 规则决定。glTF/GLB 的 data URI 和 bufferView image 会复制为 owned embedded bytes，随后走异步 texture decode。
- 保持 asset 层不依赖 GPU 资源缓存或 bindless 绑定策略

## 材质纹理与 UV

`material_texture` 定义无 GPU 所有权的 `TextureSlot<T>`、`TextureTransform` 和 sampler 描述。
四个槽按 base color、metallic-roughness、normal、emissive 顺序排列；raw 槽保存图片来源，
world 槽保存 `TextureHandle`。槽的用途决定 RGBA8 图片解释，只有图片来源与解释共同决定去重身份；
UV/transform/sampler 不参与身份。BaseColor/Emissive 为 sRGB，Normal/MR 为 Linear；FBX diffuse 使用 BaseColor 语义。

glTF 开启并解析 `KHR_texture_transform`，保持 required-extension 校验。每槽采用扩展的 `texCoord`
覆盖普通 textureInfo 的值，缺省为 UV0；offset/rotation/scale 按 `T * R * S` 求值，角度单位为弧度。
normal 通过通用扩展数据读取相同语义，`normalTexture.scale` 独立保存。

图片解码不翻转行；glTF UV 保持原值，与图片共同使用左上原点。Assimp 继续由 C++ 导入阶段的
`aiProcess_FlipUVs` 转换来源坐标；shader 不按来源格式再次翻转。
`SubmeshData` 保留连续编号的全部 UV 集及 tangent.xyzw；缺失 tangent 使用零标记，交由采样端重建。
Assimp C ABI 不提供 handedness，因此同样使用零标记，目前只导入 UV0 与 base-color/normal 图片，映射为默认值。

`SubmeshData::validate` 是导入/注册共享的属性数量、有限值、索引和 GPU uint 寻址范围校验入口。
材质引用不存在的 UV 集时导入失败，不静默退回 UV0。

## HDR / EXR 边界

- Radiance `.hdr` 和 OpenEXR `.exr` 的 `Rgb32F` / `Rgba32F` 解码结果转换为
  scene-linear RGBA16F；大于 `1.0` 的 radiance 保留，不做 gamma、曝光、tone mapping
  或色彩空间转换。普通 JPG/PNG 输出 RGBA8 原始数值，由请求解释选择 sRGB 或 UNORM 上传格式。
- 浮点纹理收到 `Srgb` 请求会失败，不静默更改解释。CPU 天空分布通过 `TextureBytes::linear_rgb()`
  读取线性数值；若读取 sRGB 数据，仅此 CPU 查询按 RGB transfer function 解码，不重写上传像素。
- RGB 非有限值与负值转为 `0`，有限值裁剪到 binary16 最大有限值 `65504`；alpha
  裁剪到 `[0, 1]`，非有限 alpha 按 `1` 处理。
- `TextureBytes` 构造阶段校验 extent 与四通道元素数量，GPU format 只由 payload
  variant 推导。image upload 与 sky distribution worker 通过 `Arc` 共享像素，不复制大型 HDR 数据。
- EXR v1 只承诺 image 解码器返回的第一张 non-deep RGB/RGBA layer 与最大 mip level，
  并把数值视为 scene-linear RGB。不支持 arbitrary channels、deep EXR、multipart
  layer 选择，也不根据 chromaticities 自动转换 ACEScg 或其它色域。
- asset 层不判断环境贴图投影；render runtime 把输入解释为 lat-long，非 2:1 只记录 warning。

材质 sampler 只有 S/T wrap 和单一 Nearest/Linear Filter。glTF 使用 magFilter（缺省 Linear），
不消费 minFilter 和 occlusionTexture；只复制四类槽引用的 embedded image，AO-only 图片不产生 texture 资源。
共享 ORM 图片仍按 MR 引用保留。此能力取舍不增加旧字段或槽索引的兼容转换。
