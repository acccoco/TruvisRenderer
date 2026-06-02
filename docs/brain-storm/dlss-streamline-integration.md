# DLSS / Streamline 接入方案

> 状态：更新于 2026-06-02。项目已经完成 Streamline runtime 布置、
> `slInit` / `slShutdown` wrapper、日志桥、`Gfx::new(...)` 默认 Vulkan
> interposer loader，以及 `Gfx::new_with_entry_source(...)` 显式 loader 入口。
> 尚未接入 DLSS feature 查询、resource tagging、common constants、
> `slEvaluateFeature`、`slFreeResources` 和 RenderGraph pass。

本文说明如何把 NVIDIA Streamline / DLSS 接入 `truvis-app` 的 RT pipeline。
当前优先级是先满足 DLSS Super Resolution（SR）的基础契约，再接入
DLSS Ray Reconstruction（RR）作为降噪路径。Frame Generation、Reflex、
NIS、DirectSR 不在本文范围内。

## 1. 目标边界

DLSS SR 和 DLSS-RR 不应混为一个 pass：

- **DLSS SR**：temporal upscaler。输入低分辨率 color、depth 或 linear depth、
  motion vectors、per-frame camera constants，输出最终分辨率 color。
- **DLSS-RR**：AI ray-tracing denoiser + upscaler。它基于 DLSS 设置运行，
  用来替代或旁路当前手写 denoise/accum 路径。除 SR 输入外，还需要材质与反射相关
  GBuffer，例如 albedo、specular albedo、normal/roughness、specular motion vectors
  或 specular hit distance。

因此，若目标是“DLSS 降噪”，实际目标应是 DLSS-RR；但 RR 依赖 SR 的尺寸、
temporal、motion vector、tagging 和 evaluate 基础设施。推荐先以 SR 跑通最小闭环，
再扩展到 RR。

官方参考：

- [Streamline Programming Guide](https://github.com/NVIDIA-RTX/Streamline/blob/main/docs/ProgrammingGuide.md)
- [DLSS Programming Guide](https://github.com/NVIDIA-RTX/Streamline/blob/main/docs/ProgrammingGuideDLSS.md)
- [DLSS-RR Programming Guide](https://github.com/NVIDIA-RTX/Streamline/blob/main/docs/ProgrammingGuideDLSS_RR.md)

## 2. 当前项目状态

已具备的基础：

- `resources.toml` 拉取 Streamline SDK 2.11.1 到 `tools/streamline-sdk/`。
- `truvis-cxx-build` 复制 DLSS SR 最小 runtime DLL 和项目维护的 `sl.*.json`。
- Debug 可额外复制 `sl.imgui.dll`，但只有 `TRUVIS_STREAMLINE_IMGUI` 显式开启时请求加载。
- `truvis-streamline-binding` 已提供进程级 `StreamlineRuntime`、日志桥和
  `StreamlineInitInfo`。
- `Gfx::new(...)` 会先初始化 Streamline runtime，再通过 `sl.interposer.dll`
  创建 Vulkan entry，保证 Vulkan root 对象处在同一条 Streamline dispatch 链中。

当前明确未覆盖：

- `slIsFeatureSupported` / `slGetFeatureRequirements` 查询。
- `slDLSSGetOptimalSettings` / `slDLSSSetOptions`。
- `slDLSSDGetOptimalSettings` / `slDLSSDSetOptions`。
- `slSetConstants`、resource tags、`slEvaluateFeature`。
- `slAllocateResources` / `slFreeResources` 和 viewport lifetime。
- RR 所需 feature flag、runtime DLL 清单与 C API。

## 3. 当前 Truvis RT Pipeline

`truvis-app` 每帧使用两个 RenderGraph：

```text
compute graph:
  ray-tracing
    -> single_frame_rt
    -> gbuffer_a / gbuffer_b / gbuffer_c
  denoise-accum
    -> accum
  blit / hdr-to-sdr
    -> main_view_color

present graph:
  resolve main_view_color -> swapchain image
  gui overlay -> swapchain image
```

已有 RT 中间资源：

- `single_frame_rt`：当前帧 noisy RT color，per-FIF。
- `accum`：progressive accumulation 的跨帧历史图。
- `gbuffer_a`：`normal.xyz + roughness`，`R16G16B16A16_SFLOAT`。
- `gbuffer_b`：`world_position.xyz + linear_depth(hit_t)`，`R16G16B16A16_SFLOAT`。
- `gbuffer_c`：`albedo.rgb + metallic`，`R8G8B8A8_UNORM`。
- `main_view_color`：present 前离屏 color，当前与 swapchain 同尺寸。

`app-kit` 的 ImGui debug image viewer 已可按当前 frame label 注册并显示这些中间图像，
用于 DLSS 接入前检查 GBuffer、depth、motion vectors 和 DLSS output。需要注意：
viewer 只负责 sampled 预览和 RenderGraph 读状态声明，normal/depth/motion vector 的
false-color 或范围映射仍应由后续专用 debug visualize pass 处理。

这个结构已经适合承载一个外部 DLSS pass，但资源语义还不满足 DLSS/RR 契约。
特别是 `GBufferB.w` 当前是 primary ray 的 hit distance，不应直接假定为 DLSS 可用的
linear depth；它必须和 motion vector 的生成方式、矩阵约定、depth range 一致。

## 4. 主要缺口

### 4.1 Streamline API 缺口

当前 binding 只到 runtime 生命周期，尚不能执行任何 DLSS feature。需要新增稳定 C ABI，
Rust 侧继续只看到 POD 结构和错误码，避免绑定 Streamline C++ ABI：

```c
truvixx_sl_is_feature_supported(feature, adapter, out_status)
truvixx_sl_get_feature_requirements(feature, out_requirements)

truvixx_sl_dlss_get_optimal_settings(options, out_settings)
truvixx_sl_dlss_set_options(viewport, options)

truvixx_sl_dlssd_get_optimal_settings(options, out_settings)
truvixx_sl_dlssd_set_options(viewport, options)

truvixx_sl_set_constants(frame_token, viewport, constants)
truvixx_sl_evaluate(feature, frame_token, viewport, cmd, resources, resource_count)
truvixx_sl_free_resources(feature, viewport)
```

第一版可只实现 DLSS SR API；RR API 和 DLL 清单在 SR 稳定后追加。

### 4.2 Vulkan 创建需求缺口

当前 `GfxDevice::basic_device_exts()` 和 `physical_device_extra_features()` 是静态列表。
DLSS 接入后需要查询 feature requirements，并为 instance/device 扩展和 feature 预留外部合并点。

建议新增 `GfxCreateDesc` 或类似结构，包含：

- app / engine name。
- Vulkan entry source。
- instance extra extensions。
- device extra extensions。
- device feature chain extension hook。

如果确认 Streamline interposer 自动补齐要求，可以先只记录 requirements 日志；一旦
validation 或 Streamline 日志提示缺项，再进入显式合并。

### 4.3 尺寸模型缺口

当前 `FrameSettings::frame_extent` 同时表示渲染分辨率和输出分辨率。DLSS 需要拆分：

```rust
pub struct FrameSettings {
    pub color_format: vk::Format,
    pub depth_format: vk::Format,
    pub render_extent: vk::Extent2D,
    pub output_extent: vk::Extent2D,
}
```

规则：

- Native / DLSS disabled：`render_extent == output_extent`。
- DLSS SR / RR enabled：`render_extent` 来自 `slDLSS*GetOptimalSettings`，
  `output_extent` 通常等于 swapchain extent。
- RT、GBuffer、depth、motion vectors 使用 `render_extent`。
- DLSS output、tone mapping、GUI、present 使用 `output_extent`。

### 4.4 Temporal 数据缺口

当前 `RenderView` 和 shader `PerFrameData` 只有当前帧矩阵。DLSS 需要稳定 temporal
常量：

- 当前和上一帧 view/projection 矩阵，且矩阵不包含 jitter。
- pixel space jitter offset。
- motion vector scale。
- depth 是否 inverted。
- reset history 标记，用于 resize、DLSS mode 切换、相机跳变、场景大变更。

现有 progressive accumulation 的 reset 可作为输入之一，但不能等同于 DLSS history reset。
矩阵传给 Streamline 前必须明确 row-major / column-major 转换，不能直接把 `glam::Mat4`
按内存布局透传。

### 4.5 Motion Vector 与 Depth 缺口

DLSS SR 最小资源：

- `kBufferTypeScalingInputColor`
- `kBufferTypeScalingOutputColor`
- `kBufferTypeDepth` 或 `kBufferTypeLinearDepth`
- `kBufferTypeMotionVectors`

当前缺少独立 motion vector image，也缺少与 motion vector 同源的可 tag depth/linear depth。
建议新增 per-FIF 资源：

```text
render_extent:
  low_res_color
  linear_depth or hardware_depth
  motion_vectors
  gbuffer_a / gbuffer_b / gbuffer_c

output_extent:
  dlss_output
  final_color_or_swapchain_source
```

motion vector 格式建议从 `R16G16_SFLOAT` 或 `R32G32_SFLOAT` 开始。第一版选择一种语义并固定：

- pixel space：`mvecScale = { 1 / render_width, 1 / render_height }`。
- normalized `[-1, 1]`：`mvecScale = { 1, 1 }`。

先做 debug view 验证，再接 DLSS evaluate。
如果 tag 的是 depth-stencil format，传给 Streamline 的 image view 必须只包含
`DEPTH` aspect，不能把 depth 和 stencil 一起作为 sampled view 暴露。

### 4.6 RR 额外资源缺口

DLSS-RR 需要 SR 输入之外的材质和反射数据：

- diffuse albedo。
- specular albedo。
- normal + roughness，可复用当前 `gbuffer_a` 的 packing，但需要设置对应 RR option。
- specular motion vectors，或 specular hit distance + 相关矩阵。
- noisy ray traced HDR input color。

当前 `gbuffer_c` 只有 `albedo + metallic`，没有 specular albedo。当前 raygen 也没有产出
specular motion vectors / specular hit distance。RR 不支持随帧动态变更输入分辨率作为常规路径；
mode/resize 变化时应视为 feature resources 重建点。

## 5. 推荐接入形态

新增 app-render-passes 层的外部 pass，而不是把 DLSS 伪装成项目 shader：

```rust
pub struct DlssRgPass<'a> {
    pub low_res_color: RgImageHandle,
    pub depth_or_linear_depth: RgImageHandle,
    pub motion_vectors: RgImageHandle,
    pub output_color: RgImageHandle,
    pub constants: DlssFrameConstants,
}
```

RR 可以在 SR 稳定后扩展：

```rust
pub struct DlssRrRgPass<'a> {
    pub noisy_color: RgImageHandle,
    pub depth_or_linear_depth: RgImageHandle,
    pub motion_vectors: RgImageHandle,
    pub diffuse_albedo: RgImageHandle,
    pub specular_albedo: RgImageHandle,
    pub normal_roughness: RgImageHandle,
    pub specular_motion_or_hit_distance: RgImageHandle,
    pub output_color: RgImageHandle,
    pub constants: DlssRrFrameConstants,
}
```

RenderGraph 只负责 pass 前后的资源状态和命令录制顺序；Streamline 负责 DLSS 内部命令。
第一版状态建议保守：

```rust
const DLSS_READ: RgImageState = RgImageState::new(
    vk::PipelineStageFlags2::ALL_COMMANDS,
    vk::AccessFlags2::MEMORY_READ,
    vk::ImageLayout::GENERAL,
);

const DLSS_WRITE: RgImageState = RgImageState::new(
    vk::PipelineStageFlags2::ALL_COMMANDS,
    vk::AccessFlags2::MEMORY_READ | vk::AccessFlags2::MEMORY_WRITE,
    vk::ImageLayout::GENERAL,
);
```

`execute()` 中需要：

- 从 `RgPassContext` 获取 `GfxImage` / `GfxImageView`。
- 组装 Streamline resource、extent、format、layout、raw Vulkan handles。
- 设置 viewport、options、common constants。
- 传入 local resource tags 调用 `slEvaluateFeature`。
- 对只在当前 evaluate 附近有效的输入使用 `eOnlyValidNow` 或 `eValidUntilEvaluate`；
  output 至少要活到后续 tone mapping / GUI / present 使用完。
- evaluate 后恢复 command buffer state。
- 失败时走 Native fallback，不让整帧崩溃。

GUI 必须在 DLSS/RR output 之后叠加，避免 UI 被 temporal upscaler / denoiser 处理。
tone mapping 的位置需要按输入契约决定：RR 要求 HDR 低分辨率 noisy input，因此 tone mapping
应在 RR output 之后。

## 6. 落地顺序

已完成：

1. SDK 资源布置、SR 最小 runtime DLL 复制、项目 `sl.*.json` 模板复制。
2. C++ wrapper 的 init/shutdown/log。
3. Vulkan entry 从 `sl.interposer.dll` 加载的默认生产路径。

下一步建议：

1. **Capability 查询**  
   增加 feature support / requirements C API。先只打印 DLSS SR requirements，不改变渲染。

2. **运行时开关与 fallback**  
   在 `PipelineSettings` 增加 upscale / denoise mode：
   `Native`、`DLSSQuality`、`DLSSBalanced`、`DLSSPerformance`、`DLAA`、
   `DLSSRayReconstruction`。unsupported 或 evaluate failed 时回退 Native。

3. **尺寸拆分**  
   引入 `render_extent` / `output_extent`。DLSS off 时保持现有行为，先不接 evaluate。

4. **Temporal 数据**  
   增加 previous matrices、jitter、history reset、mvec scale。同步更新 Rust `RenderView`、
   shader `PerFrameData` 和 Streamline constants。

5. **Motion vector / depth pass**  
   生成独立 motion vectors 和 depth/linear depth，并提供 debug view。静止相机时 motion
   vectors 应接近 0，相机移动时方向和尺度应稳定。

6. **DLSS SR evaluate**  
   新增 `DlssRgPass`，接入 low-res color、depth、motion vectors、output color。
   先以 Native fallback 保底。

7. **RR runtime 与资源扩展**  
   增加 RR feature flag、DLL 清单、`slDLSSD*` API、specular albedo、
   specular motion vectors 或 specular hit distance。

8. **DLSS-RR evaluate**  
   新增 `DlssRrRgPass`，旁路现有 `denoise-accum`。保留现有 denoise 作为 debug/fallback 路径。

9. **资源释放与 resize**  
   mode 切换、resize、shutdown 时确保 pending evaluate 已完成，再调用 `slFreeResources`。

## 7. 验证清单

基础验证：

- `just fetch-res` 后运行目录包含当前模式需要的 Streamline DLL。
- `just cxx` 能重新生成 C++ binding。
- `just truvis-direct no-validation` 和 `just truvis no-validation` 在 DLSS disabled 时行为一致。
- Streamline 日志能定位 plugin path、feature support、tagging 和 evaluate 错误。

SR 验证：

- Native 模式下 `render_extent == output_extent`。
- DLSS 模式下 render/output extent 日志与 `slDLSSGetOptimalSettings` 一致。
- motion vector debug view 在静止画面接近 0。
- jitter offset 使用 pixel space，矩阵不含 jitter。
- GUI 在 DLSS output 后叠加，文字不参与 upscaler。

RR 验证：

- RR 模式使用 HDR noisy input，tone mapping 位于 RR 之后。
- normal/roughness packing 与 `DLSSDOptions` 一致。
- albedo、specular albedo、specular motion 或 hit distance 的 debug view 可检查。
- resize 和 mode 切换会 reset history 并安全重建 feature resources。

已知需要先整理的现有问题：

- `BlitPass` 目前把 `src_image_size.y` 写成了 `dst_image_size.height`，拆分分辨率后会出错。
- `imgui/blit.slang` 当前写入红色常量，不应作为 DLSS fallback upscale 路径。
- compute graph 同时添加 `blit` 和 `hdr-to-sdr`，DLSS/RR 接入前需要明确哪个 pass 是真实输出路径，
  避免同一 target 多次写入导致语义不清。

## 8. 设计原则

- Rust 继续拥有 Vulkan root、swapchain、GPU 资源、RenderGraph 和资源生命周期。
- C++ wrapper 只包装 Streamline C++ API，不暴露 `sl::` 类型给 Rust。
- DLSS/RR 是 opaque external pass，不是项目 shader。
- app 层决定 pass 顺序；engine core 不反向持有 RT pipeline 策略资源。
- 每个阶段都必须能单独验证，避免同时修改 loader、FFI、temporal 数据和 RenderGraph。
