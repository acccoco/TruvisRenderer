# DLSS / Streamline 接入方案

> 状态：更新于 2026-06-02。当前项目已经接入 DLSS Super Resolution
> (SR) 的 Streamline 基础闭环：feature support 查询、SR mode、render/output
> extent 拆分、SR 输入资源、common constants、resource tagging、`kFeatureDLSS`
> evaluate、resize/mode reset 和 ImGui 控制。DLSS Ray Reconstruction (RR)
> 尚未接入，后续作为 SR 基础设施上的替代 evaluate 分支扩展。

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

当前 UI 和运行时只接入 SR mode：

- `Off`
- `DLAA`
- `Quality`
- `Balanced`
- `Performance`
- `Ultra Performance`

`Quality / Balanced / Performance / Ultra Performance` 由
`slDLSSGetOptimalSettings` 决定 render extent；`Off / DLAA` 使用 native extent。

RR 后续建议以 “SR mode + RR enable flag” 表达，不要把
`DLSSRayReconstruction` 写成和 `Quality/Balanced/Performance` 平级且互斥的质量挡位。
RR 复用 SR 的 Performance Quality Mode，不新增独立 RR 质量挡位。

调试启动可用：

```text
TRUVIS_DLSS_SR_MODE=off
TRUVIS_DLSS_SR_MODE=dlaa
TRUVIS_DLSS_SR_MODE=quality
TRUVIS_DLSS_SR_MODE=balanced
TRUVIS_DLSS_SR_MODE=performance
TRUVIS_DLSS_SR_MODE=ultra-performance
```

## 3. 当前 SR 落地形态

### 3.1 Streamline runtime

已完成：

- `resources.toml` 拉取 Streamline SDK 2.11.1。
- `truvis-cxx-build` 复制 DLSS SR 所需 runtime DLL 和项目维护的 `sl.*.json`。
- `StreamlineRuntime` 封装 `slInit` / `slShutdown` / 日志桥。
- `Gfx::new(...)` 默认通过 `sl.interposer.dll` 创建 Vulkan entry。
- SR support 查询通过 `slIsFeatureSupported(kFeatureDLSS)` 和
  `slGetFeatureRequirements(kFeatureDLSS)` 打日志。
- SR FFI 已封装 `slDLSSGetOptimalSettings`、`slDLSSSetOptions`、common constants、
  resource tags、`slEvaluateFeature(kFeatureDLSS)` 和 `slFreeResources(kFeatureDLSS)`。

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
- mode 切换、render extent 变化、窗口 resize 会 request SR history reset。
- 关闭 SR 时释放 viewport 0 的 DLSS resources。

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
```

DLAA 属于 `kFeatureDLSS` 分支，只是 `render_extent == output_extent`；SR upscale mode
则使用 `slDLSSGetOptimalSettings` 返回的低分辨率 `render_extent`。

`DlssSrRgPass` 是 opaque external pass：RenderGraph 只负责 pass 前后的 resource state
和命令录制顺序；Streamline 负责 DLSS 内部命令。

### 3.4 SR 资源契约

当前 SR 每帧 tag 的资源：

| Streamline tag | 当前资源 | 格式 / 尺寸 | 说明 |
| --- | --- | --- | --- |
| `kBufferTypeScalingInputColor` | `single_frame_rt` | HDR color, `render_extent` | 低分辨率 RT color。 |
| `kBufferTypeScalingOutputColor` | `dlss-sr-output` | HDR color, `output_extent` | SR 输出，后续进入 SDR。 |
| `kBufferTypeDepth` | `dlss-depth` | `R32_SFLOAT`, `render_extent` | raygen 写入 device depth；不是 `GBufferB.w` 的 hit distance。 |
| `kBufferTypeMotionVectors` | `dlss-motion-vectors` | `R32G32_SFLOAT`, `render_extent` | 第一版写 0，camera motion 由 Streamline constants 处理。 |

`GBufferB.w` 仍保留 primary-ray hit distance / linear depth 语义，供项目自身 debug 或后续 RR
扩展参考；不要把它直接当作当前 DLSS SR tag depth。

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
- `mvecScale = {1, 1}`。
- `cameraMotionIncluded = false`。
- `motionVectors3D = false`。
- `depthInverted = false`。

当前 motion vector image 只表达 object motion，第一版写 0；camera motion 交给 Streamline
根据矩阵计算。后续如果 shader 写入 camera-included motion vectors，必须同步调整
`cameraMotionIncluded`、`mvecScale` 和 debug 验证标准。

## 4. RR 后续接入点

RR 不是 SR 后追加 pass，而是替换 SR evaluate 的分支。规划层接口可保持：

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
}
```

RR 阶段需要补：

- RR runtime DLL / JSON / feature flag。
- `slDLSSDGetOptimalSettings` / `slDLSSDSetOptions` / `DLSSDOptions` FFI。
- `kFeatureDLSS_RR` support 查询、resource tagging、evaluate、free resources。
- diffuse albedo、specular albedo。
- normal + roughness packing 与 `DLSSDOptions` 一致性。
- specular motion vectors，或 specular hit distance + 相关矩阵。

RR 开启后的目标分支：

```text
single_frame_rt(noisy HDR) + GBuffer/material + depth + motion_vectors
  -> DlssRrRgPass(kFeatureDLSS_RR)
  -> dlss-sr-output 或后续重命名的 dlss-output
  -> hdr-to-sdr
```

这里的 output 可以沿用 SR 输出资源，也可以在接 RR 时重命名为更中性的
`dlss-output`；关键是不要形成 `RR -> SR` 的连续 pass。

## 5. 验证记录

当前代码验证过：

- `cargo fmt`：通过；仅有项目现有 nightly-only rustfmt 配置警告。
- `cargo check`：通过。
- `cargo build --bin truvis-app`：通过。
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
- Debug Viewer 中 `dlss-depth`、`dlss-motion-vectors`、`dlss-sr-output` 是否能按当前 frame label 查看。
- camera/object motion vector 真正接入后，静止画面应接近 0，移动方向和尺度应稳定。

## 6. 当前限制

- SR 已能 evaluate，但第一版 motion vectors 仍写 0；camera motion 依赖 Streamline constants。
- jitter 仍为 0，尚未做 temporal jitter pattern。
- fallback 策略目前以 mode 切换和错误日志为主，尚未做更细的 runtime degrade UI。
- 传统 denoise/accum pass 仍保留在代码中，但 RT 主流程已经旁路；后续可以单独清理未使用 pass。
- RR 尚未接入，不能把当前 SR 输出当作 Ray Reconstruction 结果。

## 7. 设计原则

- Rust 继续拥有 Vulkan root、swapchain、GPU 资源、RenderGraph 和资源生命周期。
- C++ wrapper 只包装 Streamline C++ API，不向 Rust 暴露 `sl::` 类型。
- DLSS SR/RR 是 opaque external pass，不是项目 shader。
- app 层决定 pass 顺序；engine core 不反向持有 RT pipeline 策略资源。
- SR 与 RR 复用基础设施，但每帧只选择一个 DLSS evaluate 分支。
