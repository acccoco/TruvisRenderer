# renderer-rendering

`renderer-rendering` 拥有与界面实现无关的 realtime/offline 渲染子系统、设置和长期 GPU 资源。

## 所有权分组

- `realtime`：`RealtimeRenderSubsystem` / `RealtimeRenderSettings`，以及 GBuffer、ReSTIR reservoir/surface key、SHARC buffer、DLSS 输入输出、working target 和 main-view target。
- `offline`：`OfflineRenderSubsystem` / `OfflineRenderSettings`，以及独立的 single-frame target、跨帧累计 image、output target、sample count 和累计签名。
- `post_process`：`TruvisRenderer` 唯一持有的 `SdrPostProcess`，拥有 AgX LUT、直方图、曝光历史与显示 pass；Realtime/Offline 通过 `SdrPostProcessInput` 借出 graph image，不转移 target 所有权。
- `shared`：`RenderMode`、`PathTracingCommonSettings`、`PathTracingDebugChannel`、`SkySamplingMode`、`SdrPostProcessSettings` 和 `ImageTarget`。
- `PathTracingDebugChannel` / `SkySamplingMode` 同时服务 realtime/offline；ReSTIR DI 和 SHARC 模式只属于 realtime 模块。
- `OfflineRenderSettings::supports_debug_channel` 是离线 debug 候选的唯一判定入口；`normalize` 将非法候选恢复为 Final 并约束 dispatch 数，由 Renderer 固定 update 路径调用，不依赖 UI。

## 生命周期与依赖

- 每个 subsystem 实现 `renderer-kit::SubsystemLifecycle`，由具体 Renderer 显式调用 `init` / `on_resize` / `shutdown` 并决定 RenderGraph pass 顺序。
- `settings()` / `settings_mut()`、`compute_cmd()` / `present_cmd()`、`contribute_compute_passes()` / `contribute_present_passes()` 和 `debug_image_options()` 保持 subsystem 自身接口。
- 窗口尺寸资源只保存 manager-owned handle；resize/shutdown 在 GPU safe point 通过 phase ctx 显式释放，不能长期保存 `Gfx`、allocator 或 runtime owner。
- offline `accum_image` 不按 FIF 轮转，不复用 realtime GBuffer、DLSS 或 ReSTIR history；只有累计签名匹配且存在 TLAS 时才推进 sample。
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
