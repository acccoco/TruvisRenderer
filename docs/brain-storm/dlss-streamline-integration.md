# DLSS / Streamline 接入方案

> 状态：更新于 2026-06-09。当前项目已经接入 DLSS Super Resolution
> (SR) 和 DLSS Ray Reconstruction (RR) 的 Streamline MVP 闭环：feature
> support 查询、SR mode + RR enable flag、render/output extent 拆分、SR/RR
> 输入资源、common constants、resource tagging、`kFeatureDLSS` /
> `kFeatureDLSS_RR` evaluate、resize/mode/feature reset 和 ImGui 控制。
> RR 第一版已经作为 SR 基础设施上的替代 evaluate 分支落地；当前仍需继续
> 验证真实反射 motion vectors、temporal jitter 和画质稳定性。

本文记录 NVIDIA Streamline 2.11.1 在 `truvis-app` RT pipeline 中的 SR/RR
接入约定。Frame Generation、Reflex、NIS、DirectSR 不在当前范围内。

官方参考：

- [Streamline Programming Guide](https://github.com/NVIDIA-RTX/Streamline/blob/main/docs/ProgrammingGuide.md)
- [DLSS Programming Guide](https://github.com/NVIDIA-RTX/Streamline/blob/main/docs/ProgrammingGuideDLSS.md)
- [DLSS-RR Programming Guide](https://github.com/NVIDIA-RTX/Streamline/blob/main/docs/ProgrammingGuideDLSS_RR.md)

## 1. SR / RR 功能关系

SR 和 RR 不是两个顺序叠加的 pass，需要按功能层面和执行层面分开理解。

- **DLSS Super Resolution (SR)**：temporal upscaler / DLAA 基础路径，可独立接入。
  SR 接收 render-resolution color、depth、motion vectors 和 common constants，输出
  final-resolution color。
- **DLSS Ray Reconstruction (RR)**：面向 ray-traced noisy signal 的 AI denoise +
  reconstruction。RR 依赖 SR 的尺寸、temporal、motion vector、resource tagging、
  common constants 和 viewport 基础设施，但不作为普通独立 denoiser 接入。
- **执行关系**：RR 开启时走 `kFeatureDLSS_RR` evaluate，取代普通
  `kFeatureDLSS` SR evaluate；它不是在 SR 前或 SR 后追加的第二个 DLSS pass。

推荐运行分支：

```text
SR-only:
  low-res color + depth + mvec + constants
    -> kFeatureDLSS
    -> high-res color

RR-on:
  low-res noisy RT color + GBuffer/material + depth + mvec + constants
    -> kFeatureDLSS_RR
    -> high-res denoised reconstructed color
```

因此，RR 开启时不要再额外运行传统 denoiser，也不要再额外跑一次 SR。

## 2. 用户可见模式

当前 UI 和运行时接入 “SR mode + RR enable flag”：

- `Off`
- `DLAA`
- `Quality`
- `Balanced`
- `Performance`
- `Ultra Performance`

`Quality / Balanced / Performance / Ultra Performance` 由
`slDLSSGetOptimalSettings` 决定 render extent；`Off / DLAA` 使用 native extent。

RR 以 “SR mode + RR enable flag” 表达，不要把
`DLSSRayReconstruction` 写成和 `Quality/Balanced/Performance` 平级且互斥的质量挡位。
RR 复用 SR 的 Performance Quality Mode，不新增独立 RR 质量挡位。`DLSS RR`
checkbox 只有在 SR mode 非 `Off` 时才会让 RenderGraph 走 `kFeatureDLSS_RR`；
`Off + RR enabled` 仍保持 native 输出。

调试启动可用：

```text
TRUVIS_DLSS_SR_MODE=off
TRUVIS_DLSS_SR_MODE=dlaa
TRUVIS_DLSS_SR_MODE=quality
TRUVIS_DLSS_SR_MODE=balanced
TRUVIS_DLSS_SR_MODE=performance
TRUVIS_DLSS_SR_MODE=ultra-performance
TRUVIS_DLSS_RR=1
```

## 3. 当前 SR 落地形态

### 3.1 Streamline runtime

已完成：

- `resources.toml` 拉取 Streamline SDK 2.11.1。
- `truvis-cxx-build` 复制 DLSS SR/RR 所需 runtime DLL 和项目维护的 `sl.*.json`。
- `StreamlineRuntime` 封装 `slInit` / `slShutdown` / 日志桥。
- `Gfx::new(...)` 默认通过 `sl.interposer.dll` 创建 Vulkan entry。
- SR/RR support 查询通过 `slIsFeatureSupported(...)` 和
  `slGetFeatureRequirements(...)` 打日志。
- SR FFI 已封装 `slDLSSGetOptimalSettings`、`slDLSSSetOptions`、common constants、
  resource tags、`slEvaluateFeature(kFeatureDLSS)` 和 `slFreeResources(kFeatureDLSS)`。
- RR FFI 已封装 `slDLSSDGetOptimalSettings`、`slDLSSDSetOptions`、
  `DLSSDOptions`、RR resource tags、`slEvaluateFeature(kFeatureDLSS_RR)` 和
  `slFreeResources(kFeatureDLSS_RR)`。

当前生产路径走 Vulkan interposer/proxy，不在 runtime 初始化后额外调用 `slSetVulkanInfo`。
`slSetVulkanInfo` wrapper 仍保留给未来非 proxy 集成方式。

### 3.2 FrameRenderState 与 resize

`FrameRenderState` 当前表达 runtime 派生的 main view 帧状态：

```rust
pub struct FrameRenderState {
    pub hdr_color_format: vk::Format,
    pub depth_format: vk::Format,
    pub render_extent: vk::Extent2D,
    pub output_extent: vk::Extent2D,
}
```

约定：

- `Off / DLAA`：`render_extent == output_extent`。
- SR upscale mode：`output_extent` 跟随 swapchain，`render_extent` 来自
  `slDLSSGetOptimalSettings`。
- RT、GBuffer、DLSS depth、motion vectors 使用 `render_extent`。
- DLSS output、SDR、GUI、present 使用 `output_extent`。
- mode 切换、RR enable flag 切换、render extent 变化、窗口 resize 会 request
  DLSS history reset。
- SR/RR feature 切换或关闭时，先等待 GPU idle，再释放旧 feature 在 viewport 0
  上的 Streamline resources。

### 3.3 RenderGraph 主流程

当前 denoise/accum pass 代码仍保留，但不在 RT 主流程中运行。Irradiance Cache 代码也保留，
但主流程 push constant 固定 `ic_enabled = 0`，让 raygen 不再依赖该路径。

```text
DLSS Off:
  ray-tracing(render_extent)
    -> single_frame_rt
    -> gbuffer_a / gbuffer_b / gbuffer_c
    -> dlss-depth / dlss-motion-vectors
  hdr-to-sdr(single_frame_rt -> main_view_color)
  resolve + gui

DLSS SR / DLAA:
  ray-tracing(render_extent)
    -> single_frame_rt
    -> gbuffer_a / gbuffer_b / gbuffer_c
    -> dlss-depth / dlss-motion-vectors
  DlssSrRgPass:
    single_frame_rt + dlss-depth + dlss-motion-vectors
      -> kFeatureDLSS
      -> dlss-sr-output(output_extent)
  hdr-to-sdr(dlss-sr-output -> main_view_color)
  resolve + gui

DLSS RR:
  ray-tracing(render_extent)
    -> single_frame_rt
    -> gbuffer_a / gbuffer_b / gbuffer_c
    -> dlss-depth / dlss-motion-vectors
    -> dlss-rr-diffuse-albedo / dlss-rr-specular-albedo
    -> dlss-rr-specular-motion-vectors
  DlssRrRgPass:
    single_frame_rt + dlss-depth + dlss-motion-vectors
      + diffuse/specular albedo + gbuffer_a(normal+roughness)
      + specular motion vectors
      -> kFeatureDLSS_RR
      -> dlss-sr-output(output_extent)
  hdr-to-sdr(dlss-sr-output -> main_view_color)
  resolve + gui
```

DLAA 属于 `kFeatureDLSS` 分支，只是 `render_extent == output_extent`；SR upscale mode
则使用 `slDLSSGetOptimalSettings` 返回的低分辨率 `render_extent`。

`DlssSrRgPass` 和 `DlssRrRgPass` 都是 opaque external pass：RenderGraph 只负责
pass 前后的 resource state 和命令录制顺序；Streamline 负责 DLSS 内部命令。RR
开启时不会再额外运行 SR pass，二者每帧只选一个 evaluate 分支。

### 3.4 SR / RR 资源契约

当前 SR 每帧 tag 的资源：

| Streamline tag | 当前资源 | 格式 / 尺寸 | 说明 |
| --- | --- | --- | --- |
| `kBufferTypeScalingInputColor` | `single_frame_rt` | HDR color, `render_extent` | 低分辨率 RT color。 |
| `kBufferTypeScalingOutputColor` | `dlss-sr-output` | HDR color, `output_extent` | SR 输出，后续进入 SDR。 |
| `kBufferTypeDepth` | `dlss-depth` | `R32_SFLOAT`, `render_extent` | raygen 写入 device depth；不是 `GBufferB.w` 的 hit distance。 |
| `kBufferTypeMotionVectors` | `dlss-motion-vectors` | `R32G32_SFLOAT`, `render_extent` | raygen 写入 pixel-space 2D motion vector，包含 camera 和 object motion；方向为 `previous_pixel - current_pixel`。 |

RR 在 SR 基础输入外额外 tag：

| Streamline tag | 当前资源 | 格式 / 尺寸 | 说明 |
| --- | --- | --- | --- |
| `kBufferTypeAlbedo` | `dlss-rr-diffuse-albedo` | `R16G16B16A16_SFLOAT`, `render_extent` | shader 从 base color / metallic 拆出的 diffuse albedo。 |
| `kBufferTypeSpecularAlbedo` | `dlss-rr-specular-albedo` | `R16G16B16A16_SFLOAT`, `render_extent` | shader 按项目 PBR 约定拆出的 specular albedo。 |
| `kBufferTypeNormalRoughness` | `gbuffer-a` | `R16G16B16A16_SFLOAT`, `render_extent` | `normal.xyz + roughness`，`DLSSDOptions.normalRoughnessMode = Packed`。 |
| `kBufferTypeSpecularMotionVectors` | `dlss-rr-specular-motion-vectors` | `R32G32_SFLOAT`, `render_extent` | raygen 沿 primary hit 的镜面反射方向追踪虚拟几何后写入 pixel-space 2D motion；未命中时写 0。 |

`GBufferB.w` 仍保留 primary-ray hit distance / linear depth 语义，供项目自身 debug 或后续
RR 扩展参考；当前 SR/RR 都 tag `dlss-depth` 作为 device depth，不直接把 `GBufferB.w`
交给 Streamline。

SR 输入在 RenderGraph 中使用 `DLSS_SR_INPUT_READ`：

```rust
RgImageState::new(
    vk::PipelineStageFlags2::ALL_COMMANDS,
    vk::AccessFlags2::MEMORY_READ,
    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
)
```

DLSS output 之后会被 `hdr-to-sdr` 作为 storage image 读取，因此 output 保持 `GENERAL`。
ImGui debug image viewer 会按当前 selected image 的稳定 graph state 重新 import，避免把
SR 输入误按 `GENERAL` 导入 present graph。

### 3.5 Common constants

`DlssSrState` 负责维护 Streamline common constants：

- current view/projection 与 inverse projection。
- current clip 和 previous clip 的变换。
- camera position/right/up/forward、near/far/fov/aspect。
- reset history 标记。
- `jitterOffset = 0`。
- `mvecScale = {1 / render_width, 1 / render_height}`。
- `cameraMotionIncluded = true`。
- `motionVectors3D = false`。
- `depthInverted = false`。

当前 motion vector image 写入完整 2D screen motion。runtime 会在 `PerFrameData`
写入 previous view/projection，在 `Instance` 写入 `prev_model`；新激活实例、resize、
DLSS mode/RR 切换或 history reset 帧会把 previous 数据对齐到当前帧，避免第一帧脏向量。
后续接 temporal jitter 时，需要同步 motion vector 是否包含 jitter delta 与
`motionVectorsJittered` / jitter offset 契约。

## 4. RR 当前落地与剩余缺口

RR 不是 SR 后追加 pass，而是替换 SR evaluate 的分支。当前 RenderGraph 接口为：

```rust
pub struct DlssRrRgPass<'a> {
    pub input_color: RgImageHandle,
    pub output_color: RgImageHandle,
    pub depth: RgImageHandle,
    pub motion_vectors: RgImageHandle,
    pub diffuse_albedo: RgImageHandle,
    pub specular_albedo: RgImageHandle,
    pub normal_roughness: RgImageHandle,
    pub specular_motion_vectors: RgImageHandle,
}
```

已落地：

- RR runtime DLL、feature flag 和 support 查询。
- `slDLSSDGetOptimalSettings` / `slDLSSDSetOptions` / `DLSSDOptions` FFI。
- `kFeatureDLSS_RR` support 查询、resource tagging、evaluate、free resources。
- `DlssRrPass` / `DlssRrRgPass` opaque external pass。
- `DlssRrInputTargets` 管理 diffuse albedo、specular albedo、specular motion vectors。
- raygen 写出 RR 所需 diffuse/specular albedo；normal + roughness 复用 GBufferA。
- raygen 写出 primary full-screen motion vectors，并用一次反射 `RayQuery` 写出
  RR specular motion vectors；反射未命中时使用零向量作为保守 fallback。
- SR/RR feature 切换时释放旧 feature resources，避免 Streamline viewport resource
  跨 feature 残留。

仍需继续补齐或验证：

- temporal jitter 当前仍为 0；后续接 DLSS 推荐 jitter pattern 时，需要同步 raygen、projection
  constants、history reset 和 debug 验证。
- specular motion vectors 当前采用 single reflection RayQuery，不处理透明材质、粒子或多层反射；
  后续如需要更高质量，可补 specular hit distance / 多 bounce 策略。
- 当前 output 仍沿用 `dlss-sr-output` 资源名；功能正确但命名偏 SR，后续可以改为
  `dlss-output`，避免 debug UI 误读。
- `slDLSSDGetOptimalSettings` FFI 已暴露，但 runtime 仍复用 SR optimal settings 计算
  render extent。若 SDK/driver 对 RR optimal settings 有额外约束，应再切到 RR-specific query。

## 5. 验证记录

当前代码验证过：

- `cargo fmt`：通过；仅有项目现有 nightly-only rustfmt 配置警告。
- `clang-format -i ...`：通过。
- `cargo run --bin shader-build`：通过。
- `cargo build -p truvis-shader-binding`：通过。
- `cargo run --bin cxx-build`：通过，Debug/Release 输出均复制 SR/RR runtime DLL。
- `cargo build -p truvis-assimp-binding -p truvis-streamline-binding`：通过。
- `cargo check --all`：通过。
- `cargo build --bin truvis-app`：通过。
- `cargo build --bin rt-cornell`：通过。

既有 SR 运行时回归记录（本轮 RR 接入后尚未重跑交互式窗口验证）：

- DLSS Off + Vulkan validation + 窗口 resize：退出码 0，未扫到 `VUID` / validation error。
- `TRUVIS_DLSS_SR_MODE=quality` + Vulkan validation + 窗口 resize：退出码 0，未扫到
  `VUID` / validation error / `DLSS SR evaluate failed`。

SR 回归检查关键字：

```text
Ray Reconstruction
Super Resolution
kFeatureDLSS
kFeatureDLSS_RR
DLSSD
```

运行时需要继续关注：

- SR mode 切换后 render/output extent 日志是否符合 `slDLSSGetOptimalSettings`。
- resize 后是否触发 DLSS history reset 和 target rebuild。
- Debug Viewer 中 SR/RR 输入与 `dlss-sr-output` 是否能按当前 frame label 查看。
- camera/object/specular motion vector 静止画面应接近 0，移动方向和尺度应稳定。
- RR 开启时日志中不应出现 `DLSS RR evaluate failed`，且不应再出现同一帧连续 SR evaluate。

## 6. 当前限制

- jitter 仍为 0，尚未做 temporal jitter pattern。
- motion vectors 已接入 full-screen 2D motion，但尚未做 runtime 可视化量纲校验和 DLSS 画质回归。
- fallback 策略目前以 mode 切换和错误日志为主，尚未做更细的 runtime degrade UI。
- 传统 denoise/accum pass 仍保留在代码中，但 RT 主流程已经旁路；后续可以单独清理未使用 pass。
- RR output 资源名仍为 `dlss-sr-output`，语义上已经按当前 feature 分支区分，命名后续可清理。

## 7. 设计原则

- Rust 继续拥有 Vulkan root、swapchain、GPU 资源、RenderGraph 和资源生命周期。
- C++ wrapper 只包装 Streamline C++ API，不向 Rust 暴露 `sl::` 类型。
- DLSS SR/RR 是 opaque external pass，不是项目 shader。
- app 层决定 pass 顺序；engine core 不反向持有 RT pipeline 策略资源。
- SR 与 RR 复用基础设施，但每帧只选择一个 DLSS evaluate 分支。
