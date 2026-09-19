# 材质纹理颜色空间归一化

> 状态：方案已实施（2026-09-18）；保留同条件画面对照的验收边界。当前实现事实见 [Scene 数据生命周期](../summaries/scene-data-lifecycle.md)。

## 目标

纹理用途决定输入数据的颜色空间，GPU 采样后统一得到可直接参与线性材质计算的数值。保留图片解码后的 8-bit RGBA 数值，由 sRGB image/view 对颜色纹理的 RGB 进行采样解码；不在 CPU 预解码后重新量化成线性 8-bit，也不在 shader 中另做 gamma 转换。

| 输入 | 解释与目标格式 | 后续计算 |
| --- | --- | --- |
| glTF BaseColor、Emissive | RGB 为 sRGB；RGBA8 使用 `R8G8B8A8_SRGB` | BaseColor 乘线性 `baseColorFactor`；Emissive 乘线性发光因子 |
| FBX Diffuse | 项目导入约定为 sRGB；RGBA8 使用 `R8G8B8A8_SRGB` | 使用已导入的材质颜色因子，恰好乘一次 |
| glTF Normal、MetallicRoughness，FBX Normal | 线性数据；RGBA8 使用 `R8G8B8A8_UNORM` | 按现有槽和通道读取，不做 sRGB 解码 |
| 其他纹理，包括程序化注册和 LDR 天空 | 默认线性；RGBA8 使用 `R8G8B8A8_UNORM` | 按各自已有的使用场景处理 |
| HDR/EXR | 线性浮点；`R16G16B16A16_SFLOAT` | 保留大于 1 的辐亮度；不强制转为 UNORM |

glTF 的槽级解释与 factor 语义来自 [glTF 2.0 规范](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html)；FBX Diffuse 的 sRGB 解释是本项目的明确策略，不从文件名或图片 metadata 猜测。glTF PNG/JPEG 自带的 ICC、gAMA 等颜色空间信息不覆盖 glTF 槽语义。sRGB 格式只将 RGB 转为线性数值，Alpha 仍按原数值读取，见 [Vulkan image reads](https://docs.vulkan.org/spec/latest/chapters/images.html)。

### 必须保持的边界

- `TextureAssetHandle` 指向一种确定的上传格式。同一来源同时作颜色图和数据图时，它们是两个不同的 texture handle；相同来源且相同解释则复用。不得因导入或编辑顺序改变既有 handle 的解释。
- `TextureLoadDesc` 仅为一次任务的输入；长期身份仍由 `AssetSystem` 管理。`TextureBytes` 的像素表示与上传格式必须一致，不让调用方另传可与像素不符的 Vulkan format。
- 纹理槽保留 UV、transform、sampler；颜色空间不进入 `MaterialTexture` shader ABI。材质最终取其纹理 handle 对应的唯一 SRV；没有槽级双 view 选择开关。
- CPU `AssetSystem` 拥有权威资源数据；GPU image/view、bindless descriptor、迟到完成及 FIF 退役仍由 RenderThread 上的 `RenderAssetSystem` 负责。现有 prepare 顺序、线程边界和 RenderGraph pass 顺序不变。
- BaseColor RGB = 线性采样 RGB × 线性材质 RGB factor；Alpha = 纹理 Alpha × 材质 Alpha factor。无贴图时采样中性值 1。Emissive 沿用独立发光因子乘法，Normal/MR 的语义不变。

## 实施前基线与已清理差异

`GltfSceneReader` 和 `TruvixxSceneReader` 原已输出四种 `TextureChannel`；FBX diffuse 对应 BaseColor。实施前，`AssetSystem` 仅按图片来源去重；`TextureLoadDesc` 不携带解释；普通图片统一上传为 UNORM，`GpuMaterialStore` 以槽索引选择 sRGB view。这些旧契约现已移除。

此前亮度排查实验跳过 base-color RGB factor，并仅发布 UNORM view；现已恢复统一 factor 乘法，`phong.ps.slang` 继续复用 `MaterialAccess::base_color_rgba`，实验说明已从活跃 summary 删除。

## 已实施方案

### 1. 在现有 owner 中表达纹理解释

在 `truvis-asset` 定义仅有 `Srgb`、`Linear` 的 `TextureColorSpace`；把解释作为 `TextureLoadDesc` 的显式请求参数，File 与 Embedded 路径使用同一契约。用 `TextureChannel` 的一个现有职责方法统一映射：BaseColor/Emissive → `Srgb`，Normal/MetallicRoughness → `Linear`。glTF/FBX 导入仍输出现有四槽，不创建第二套 importer 配置或按后缀判断颜色空间的 helper。

`AssetSystem::register_scene_texture` 使用当前槽传来的解释注册资源。程序化 `GameWorld::import_texture` 显式接收解释；当前应用的 LDR 天空和 `request_sky_texture_from_path` 传 `Linear`。Editor 当前只编辑已存在纹理槽的映射参数，不提供创建或替换纹理的命令，因而不新增 Editor DTO/协议或界面选项。将来若增加该功能，必须使用同一显式注册入口。

### 2. 使资源身份与上传数据一致

`AssetSystem` 的外部路径键改为 `(canonical path, TextureColorSpace)`，内嵌键改为 `(scene path, image id, TextureColorSpace)`；对应的查找、遗忘、删除和异步完成映射一并更新。同一 glTF image 同时用于 BaseColor 与 MR 时形成两个 handle、两次解码和上传；先接受这点少量资源开销，不引入共享 encoded bytes 数据库、按 view 分叉的资源所有权或内容哈希。删除任一变体时仍按现有引用检查和 FIF 退役，不牵连另一变体。

`TexturePixels` 用一个 RGBA8 payload 加颜色空间字段、一个 RGBA16F payload 表达格式差异；避免分别实现两套相同的字节读取、extent 校验和生命周期逻辑。`TextureBytes::format()` 据此选择 `R8G8B8A8_SRGB`、`R8G8B8A8_UNORM` 或 `R16G16B16A16_SFLOAT`。RGBA8 的像素字节保持图片解码后的值；浮点 HDR/EXR 必须为 `Linear`，若收到 `Srgb` 请求则明确失败，而非悄悄改变资源解释。用于天空分布的 `TextureBytes::linear_rgb()` 应按自身格式返回线性 RGB，不能把 sRGB 原始字节误当作线性辐亮度；当前天空仍按 Linear 注册。

### 3. 让 GPU 与 shader 只消费确定的资源

`GpuAssetUploadQueue` 按 `TextureBytes::format()` 创建 image；RGBA8 不再无条件使用 `MUTABLE_FORMAT`。`GpuTextureStore` 每个 handle 只注册一种同格式 view/SRV，并只注销这一个 binding。`TextureResolver::resolve_texture(handle)` 去掉 `srgb: bool`，材质与天空统一按 handle 解析；`GpuMaterialStore` 不再通过槽索引推断 GPU view 类型。Fallback 与现有 texture ready、binding revision 和延迟销毁契约保持一致。

恢复 `MaterialAccess::base_color_rgba` 对 RGB/Alpha 的 factor 乘法，Realtime、Offline、Phong 继续共用这一入口；Emission 继续由独立的 factor 与采样值相乘。FBX 的 Assimp `AI_MATKEY_COLOR_DIFFUSE` 已作为内部 `base_color` 导入，不另取或再乘一次 FBX `DiffuseFactor`。不增加 shader sRGB 数学分支，也不改变共享 shader ABI。

### 4. 同步活跃事实文档

实施完成后更新 `docs/summaries/scene-data-lifecycle.md`、`docs/summaries/realtime-rt-raytracing-flow.md`、`truvis-asset/README.md` 和直接描述 image/view 所有权的 runtime 文档；修正 `docs/brain-storm/fixtures/README.md` 中的图片身份和验收契约：原 fixture 的颜色/MR/emissive 共用图片来源，目标状态下应按颜色空间形成相应两个 handle。移除关于双 view、全 UNORM 实验和“不乘 factor”的过期说明。设计落实后只在有后续决策价值时保留本文，否则把稳定契约提炼到 summary 并删除本文，不建归档。

## 实施顺序与验证

以下按可审查的行为变化推进，每步在其依赖已经落地后进行；不为旧 API 或临时实验保留兼容层。

1. **CPU 语义与身份**：增加显式颜色空间请求，更新四槽映射、File/Embedded 去重键、`GameWorld`/天空注册入口、异步完成与删除路径。检查同源同义复用、同源异义区分以及迟到完成不会串写两个 handle。
2. **像素与 GPU 资源**：让 loader 产出带格式语义的 `TextureBytes`，上传为唯一 image/view/SRV；移除 mutable format 和双 view 选择参数。用已知像素核对图片解码器没有隐式应用 PNG/JPEG 的 ICC/gAMA 转换；检查 RGBA8 字节数、sRGB Alpha、HDR/EXR >1 及天空分布的线性输入。
3. **材质计算与实验清理**：恢复 BaseColor factor 恰好乘一次，保留 Raster/RT 共用的 `MaterialAccess`，删除诊断逻辑和旧文档描述。检查 glTF factor=1 与非 1、FBX diffuse factor、缺贴图 fallback 和 Emissive。
4. **构建与画面对照**：依次执行 `cargo check` 覆盖受影响的 asset/world/runtime/renderer crate、`just shader` 以及项目已有的编辑器构建检查；用现有材质 fixture 比较颜色图、MR/Normal 共源图、alpha MASK 和实时/离线/Phong。用同相机、同天空、相同累计条件对比 Sponza FBX 与 glTF 场景。编译和静态检查不替代实际 GPU 像素证据。

每个阶段应单独说明修改前后的行为和剩余风险；若拆为提交，遵循项目每次提交一种可验证行为变化、独立编译回滚的规则。项目约定不主动增加单元测试；优先复用已有 fixture 与构建、运行验收路径。

## 边界与完成标准

不恢复材质 mipmap 或 Occlusion 槽，不引入图像 ICC 管线、FBX 自动颜色空间猜测、编辑器新协议、纹理热替换、第二套颜色/数据 sampler、按每个材质槽动态切换同一 image 的 view、CPU 全图 sRGB 转码或新增零散 free function。若将来要支持按图片 metadata 解释 LDR 天空，应作为明确的新需求处理。

完成时：两个导入器和程序化入口均能确定唯一解释；同图跨用途不会受注册顺序影响；RGBA8 颜色贴图采样得到线性 RGB、线性数据图不被解码、Alpha 不经 sRGB 转换；BaseColor factor 在所有路径乘一次；异步 ready/回收和天空分布正常；工作区的临时亮度实验与过期文档已清理，相关 `docs/summaries/` 反映实际代码。GPU 运行结果未取得前，只报告静态与构建验证，不声称画质问题已经解决。

## 2026-09-18 实施与验收记录

- 已落实 File/Embedded 的显式颜色空间和带解释的长期去重键；CPU 夹具导入得到 18 个 instance、两张图片来源、三个 handle。颜色与 Emissive 同源复用，颜色与 MR 异义分离，Normal 使用另一张图。原夹具及忽略 Occlusion 的变体均通过现有手工集成检查。
- RGBA8 保留解码后的字节，按解释建立唯一 sRGB/UNORM image/view；浮点仍为线性 RGBA16F，sRGB 浮点请求失败。检查所用 `image` crate 的 PNG 解码路径没有隐式 ICC/gAMA 颜色变换；不同格式的全面 metadata 对照未做。
- 统一 BaseColor factor 和 Alpha factor 乘法，保留 RT/Phong 共用入口；移除了诊断性的无 factor 路径、双 view 和 mutable-format 上传。编译检查、既有 6 项 asset/world 测试、26 项 shader 编译、Web 编辑器构建与应用构建均通过。
- Vulkan 实际加载原夹具的 18 个 instance；用已有的无遮挡 sampler 变体和独立像素脚本，Realtime BaseColor 的 450 个点中 360 个误差不超过 2/255，前四列的 12 个 panel 均为 25/25。再把 panel 0 的 factor 改为 `[0.5, 0.25, 0.75, 0.6]`，其 25/25 个 RGB 点继续符合“sRGB 解码后乘 factor”的计算。其余 panel 部分区域呈现白色缺口或与预期位置不同，尚不能判断原因，未通过全画面对照。默认 Sponza 应用也已实际启动并加载；运行日志未发现本次 Vulkan validation error。

后续如需确认原先的整体亮度问题，应先定位上述参考场景的白色缺口/位置差异，再在相同相机、天空、曝光及累计条件下对比 FBX、glTF 的 Realtime/Offline/Phong 实际画面；本轮不把编译通过或局部像素匹配等同于这些视觉结果。
