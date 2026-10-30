# renderer-rendering

RR 输入使用单通道 specular hit distance，生命周期由 `DlssRrInputTargets` 管理；不保留 specular MV。
shader/FFI 契约、反射距离近似与 debug 规则见
[`render-configuration-and-temporal-state.md`](../../docs/design/render-configuration-and-temporal-state.md)。

`renderer-rendering` 拥有与界面实现无关的 realtime/offline 渲染子系统、设置和长期 GPU 资源。

## 所有权分组

- `realtime`：`RealtimeRenderSubsystem` / `RealtimeRenderSettings`，以及 GBuffer、ReSTIR reservoir/surface key、SHARC buffer、DLSS 输入输出、working target 和 main-view target。
- `offline`：`OfflineRenderSubsystem` / `OfflineRenderSettings`，以及独立的 single-frame target、跨帧累计 image、output target、sample count 和累计签名。
- `post_process`：`TruvisRenderer` 唯一持有的 `SdrPostProcess`，拥有 AgX LUT、直方图、曝光历史与显示 pass；Realtime/Offline 通过 `SdrPostProcessInput` 借出 graph image，不转移 target 所有权。
- `shared`：`RenderMode`、`PathTracingCommonSettings`、`PathTracingDebugChannel`、`SkySamplingMode`、`SdrPostProcessSettings` 和 `ImageTarget`。
- `PathTracingDebugChannel` / `SkySamplingMode` 同时服务 realtime/offline；ReSTIR DI 和 SHARC 模式只属于 realtime 模块。
- DLSS 的 pass adapter、options、SR frame constants 和 snapshot 位于 `renderer-render-passes`；具体 requested/effective 配置、能力与资源生命周期由 `TruvisRenderer` 持有。
- 环境倍率由 World 持有，Realtime/Offline 从同帧 RenderSceneView 读取；不在 common settings 保存副本。
- `OfflineRenderSettings::supports_debug_channel` 是离线 debug 候选的唯一判定入口；`normalize` 将非法候选恢复为 Final 并约束 dispatch 数，由 Renderer 固定 update 路径调用，不依赖 UI。

## 生命周期与依赖

- 每个 subsystem 实现 `renderer-kit::SubsystemLifecycle`，由具体 Renderer 显式调用 `init` / `on_resize` / `shutdown` 并决定 RenderGraph pass 顺序。
- `settings()` / `settings_mut()`、`compute_cmd()` / `present_cmd()`、`contribute_compute_passes()` / `contribute_present_passes()` 和 `debug_image_options()` 保持 subsystem 自身接口。
- 窗口尺寸资源只保存 manager-owned handle；resize/shutdown 在 GPU safe point 通过 phase ctx 显式释放，不能长期保存 `Gfx`、allocator 或 runtime owner。
- Realtime/Offline 的 resolve pipeline 在 resize 时核对当前 present format；仅格式变化才创建替代并释放旧 pipeline，普通尺寸变化继续复用。
- offline `accum_image` 不按 FIF 轮转，不复用 realtime GBuffer、DLSS 或 ReSTIR history；签名变化可立即失效，样本计数仅在相应 batch 提交返回后推进。
- Realtime/Offline 建图返回各自的提交候选，由 Renderer 在 queue submit 正常返回后消费；丢弃图或提交失败不推进计数/历史。无 TLAS 的清屏也须提交后才确认清零/断开连续帧；这不是 GPU 完成凭据。
- Offline 在整个采样 batch 的样本数超出 u32 前明确停止；不饱和重复索引，也不自动清空历史。传入 accum shader 的历史数始终小于 u32::MAX，保证分母加一可表示。
- GBuffer 三通道复用 shared `PerFrameImageSet` 的 image/view 生命周期；通道格式、usage 和几何/材质含义仍由 GBuffer 定义。
- ReSTIR surface key 使用独立的四张 RGBA32F，保留位置/深度、着色法线/roughness、base_color/metallic、几何法线；current/history 与 reservoir 使用相同 FIF label。图像由 `RestirDiTargets` 拥有，普通 surface descriptor set 由 RT pass 拥有；不复用压缩 GBuffer。
- 主视图与 Offline 中间输出只保留 storage、采样和传输用途；Resolve 的 color attachment 位于 swapchain。主视图不分配未被 pass 消费的 raster depth，DLSS depth 仍由自己的 input target 提供。
- Target 与 Realtime/Offline RenderResources 由 owner 在 GPU safe point 显式销毁；不做逐层 unwind 回滚。
- SHARC 保留三个 cache buffer 和两个 Resolve snapshot；clear 只初始化 cache，snapshot 在 Resolve 前完整覆盖，resize 不重建世界空间 cache。
- SHARC 根据全部 scene accumulation 版本、scene scale 和 NEE 配置决定语义 reset，签名只随提交候选确认；Off / 无 TLAS 提交切断历史，Update↔On、相机与 resize 可复用。RT pass 在 Update 前录制三个 cache 的 fill 及 RT↔Clear 依赖；不额外 wait，不清理即将全量覆盖的快照。边界见 radiance-cache 设计文档。
- SHARC hash capacity 为零时 Insert/Find 返回无效索引；生产容量固定为 `2^22`。17-bit position、10-bit level、u32 accumulation 和 fp16 resolved 的溢出、饱和与坐标范围属于当前表示限制，不能由普通 shader 编译验证替代数值与 GPU 验收。
- SDR 的初始化、模式与 pending snap 同样通过同次 render 的提交候选确认；建图不消耗状态，无输入/锁定/非 Final 不会提前消费首次有效测光。
- LUT 上传复用 `GfxImage::transfer_data` 的同队列 shader 读取可见性；同步上传完成后释放 staging，不重复追加 compute 屏障。
- Realtime debug preview 只借出当前生产路径有效的 source：无 TLAS 时仅提供已清理的 source/main color/hit-distance，DLSS output 须本帧 evaluate 成功返回；非空 handle 本身不是内容有效凭据。
- 依赖 `renderer-kit`、`renderer-render-passes` 和 Engine 渲染能力，不依赖 ImGui；SDR 设置通过 shared API 暴露，避免 UI 直接依赖 pass crate。

## 显示契约

默认 Khronos PBR Neutral + Manual，手动增益 0 stops、中性 Color Grading、dithering 开启；可选择 ACES fitted、AgX、None 和 Auto。
Manual 只使用手动增益；自动补偿仅属于 Auto，默认 0 stops。
Auto 默认中心加权测光，Realtime 按时间适应，Offline 直接测量当前 HDR 累积均值；曝光锁定仅冻结自动增益。
所有显示设置都不重置 Offline spp。
`PathTracingCommonSettings::post_process` 是唯一设置入口；不保留旧设置名称或兼容参数。

AgX 和 ACES fitted 参考 Bevy commit `566358363126dd69f6e457e47f306c68f8041d2a` 的
[`tonemapping_shared.wgsl`](https://github.com/bevyengine/bevy/blob/566358363126dd69f6e457e47f306c68f8041d2a/crates/bevy_core_pipeline/src/tonemapping/tonemapping_shared.wgsl)。
AgX 保留输入矩阵、相对中灰 [-10,+6.5] log 范围和 default-contrast LUT；ACES 保留输入/输出矩阵
和 RRT/ODT 拟合，不表达完整 ACES/OCIO。LUT 来源、许可、校验值与无损 atlas 转换见
[`resources/README.md`](resources/README.md)。shader 的矩阵使用逐行 dot 明确方向。

直方图参考同版本 [`auto_exposure.wgsl`](https://github.com/bevyengine/bevy/blob/566358363126dd69f6e457e47f306c68f8041d2a/crates/bevy_post_process/src/auto_exposure/auto_exposure.wgsl)，
适配为 256 bins、image 资源、精确边界权重与按 dt 的指数适应。设置、历史 reset 与锁定语义见
[`render-configuration-and-temporal-state.md`](../../docs/design/render-configuration-and-temporal-state.md)。

`PathTracingDebugChannel::is_radiance` 是唯一分类入口：Final、NEE/BRDF/emission 贡献、ReSTIR final
contribution、SHARC cache radiance 使用同一显示变换；仅 Final 更新测光。法线、BaseColor、mask、
权重和 heatmap 不应用曝光、调色、曲线或 dithering。Debug Image 原始资源查看器仍表达选中资源自身的数值。
SHARC cache debug 在 Off 不读历史；hash/cache 两个表面通道在 primary miss 输出零。
原始语义以 single-frame-rt 为准，DLSS 开启后仍可能影响最终显示；debug 不单独 reset 时域状态。
显示变换非线性，贡献相加必须在线性 HDR 中验证；显示增强不等同于降低玻璃焦散的采样方差。

Color Grading 融合在当前 SDR pass，按白平衡、对比度、明暗分区、饱和度顺序执行。
白平衡与分区参考 Unity Graphics commit `03ca85dffdde4b7bc1d6870074e6f5ff9f0352a3` 的
[ColorUtils](https://github.com/Unity-Technologies/Graphics/blob/03ca85dffdde4b7bc1d6870074e6f5ff9f0352a3/Packages/com.unity.render-pipelines.core/Runtime/Utilities/ColorUtils.cs)、
[Color.hlsl](https://github.com/Unity-Technologies/Graphics/blob/03ca85dffdde4b7bc1d6870074e6f5ff9f0352a3/Packages/com.unity.render-pipelines.core/ShaderLibrary/Color.hlsl) 与
[LutBuilderHdr.shader](https://github.com/Unity-Technologies/Graphics/blob/03ca85dffdde4b7bc1d6870074e6f5ff9f0352a3/Packages/com.unity.render-pipelines.universal/Shaders/PostProcessing/LutBuilderHdr.shader)。
CPU 合成白平衡矩阵与标量分区增益，shader 使用固定 Rec.709 亮度；对比度是本项目中灰亮度公式，
不照搬 Unity 的 LUT 框架或随映射切换的调色空间。温度使用相对偏移，调色不恢复丢失细节、不抬升纯黑，
负通道归零且没有完整广色域映射；参数和历史规则见 temporal-state 设计文档。

Khronos PBR Neutral 参考 commit `b5a2eed5ddf6c2227090449399de9c7affb9e4c9` 的
[pbrNeutral.glsl](https://github.com/KhronosGroup/ToneMapping/blob/b5a2eed5ddf6c2227090449399de9c7affb9e4c9/PBR_Neutral/pbrNeutral.glsl)，
Copyright (c) 2024 The Khronos Group, Inc.，采用 Apache-2.0（见现有
[LICENSE-APACHE](resources/LICENSE-APACHE)）。从 GLSL 改写为 Slang 关联方法，保留常量与解析公式，
不增加 LUT；输入和输出均为 linear Rec.709，输出范围 [0,1]。所有模型参数固定，None 只跳过显示映射。
