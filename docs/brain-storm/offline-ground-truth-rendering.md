# 离线 Ground Truth 渲染管线

> 状态：首版已实现，更新于 2026-06-20。当前事实以
> [`docs/ARCHITECTURE.md`](../ARCHITECTURE.md)、[`docs/summaries/`](../summaries/) 和代码为准。

本文记录离线 ground truth 渲染管线的目标、当前实现、资源边界、RenderGraph 接入方式和后续扩展点。

## 最终目标

新增一条独立于实时 RT 主流程的离线渲染管线，用来在 GPU 上逐步累计 reference / ground truth 结果，并直接通过现有 present 路径显示最新累计后的画面。

已落地的目标形态：

- `RenderMode { Realtime, Offline }` 是 app-kit 共享渲染模式枚举，默认 `Realtime`。
- Truvis 的 Controls 面板提供 `Render Mode` combo；只有传入离线 pipeline 设置的 app 才显示该模式切换。
- `OfflinePipeline` 贡献自己的 compute sub RenderGraph 和 present sub RenderGraph，不复用实时 `RtPipeline` 的 DLSS、ReSTIR 或 RR 分支。
- 离线结果累计到 pipeline-owned 的 FIF 唯一 `accum_image`，不是 per-FIF image，也不是 Vulkan buffer。
- 曝光 / tone mapping 以 `accum_image` 为输入，输出到 per-FIF `render_target`。
- present sub RenderGraph 在离线模式下读取最新的离线后处理结果；不导出图片，不写磁盘文件。

离线 v1 的非目标：

- 不接入 ReSTIR DI、ReSTIR GI、DLSS SR、DLSS RR 或 denoise。
- 不把实时 motion vector、DLSS input、RR input 或 ReSTIR reservoir 纳入离线资源。
- 不把 runtime `ViewAccumState` 当作离线 sample count；离线累计状态独立维护。
- 不改变实时 `RtPipeline` 的默认路径和现有 DLSS / ReSTIR 调试能力。

## 当前数据流

离线 v1 采用“单帧输出 + 唯一累计图像 + per-FIF 显示目标”资源拓扑：

```text
RenderSceneView / TLAS / Scene buffers / Light tables / Sky
        |
offline ray tracing pass x 1..8
        |
per-FIF single_frame_image
        |
accum pass x 1..8
        |
FIF 唯一 accum_image
        |
hdr-to-sdr / tone mapping pass
        |
per-FIF render_target
        |
present sub RenderGraph
        |
swapchain
```

资源约定：

- `single_frame_image`：per-FIF HDR storage image，保存本帧离线 path tracing 输出。
- `accum_image`：FIF 唯一 HDR storage image，保存 progressive accumulation 后的线性 HDR reference 结果。
- `render_target`：per-FIF HDR image，保存可交给 present graph 的 tone mapped 结果。
- `accum_image` 使用 `FrameRenderState::hdr_color_format`；当前 runtime 默认值是 `R32G32B32A32_SFLOAT`。
- `accum_image` 导入 RenderGraph 时使用 `RgImageState::STORAGE_READ_WRITE_COMPUTE` 作为初始状态。
- `render_target` 在离线 compute graph 末尾导出为 `RgImageState::SHADER_READ_FRAGMENT`，present graph 再按相同状态导入。
- 如果当前 frame label 没有 TLAS，离线 graph 会 reset sample count，不调度 `OfflineRtPass` / `AccumPass`，
  而是用 `ImageClearPass` 把三张离线图像写成黑色确定输出；该路径不会把 stale single-frame 结果累计进 `accum_image`。

算法约定：

- 积分器使用 path tracing，每帧按 `OfflinePipelineSettings.ray_dispatch_count` 推进 1-8 个样本；每个样本对应一次 Vulkan
  `TraceRays` dispatch 和一次 `AccumPass` 融合。
- primary ray jitter 由 `OfflineAccumState` 按离线 sample index 生成 Halton 2/3 序列，单位是 pixel；
  它不读取 `PerFrameData::temporal_jitter_px`，因此不受 DLSS / DLAA 开关影响。
- raygen 只写 `single_frame_image`，不写 realtime GBuffer、motion vector、DLSS input 或 reservoir。
- NEE 默认开启，复用当前 realtime RT 已整理出的 HDRI、emissive triangle 和 analytic light helper。
- MIS 继续覆盖 NEE、sky miss 和 emissive hit 的竞争估计。
- 最大路径深度、Russian roulette 起始深度等仍沿用 shader helper 里的 v1 常量；后续如需 UI 设置再扩展 `OfflinePipelineSettings`。
- v1 使用现有 online mean 累积公式；后续如果要支持 variance、adaptive sampling 或更精确统计，再扩展累计格式。

## 当前接口与状态边界

`RenderMode` 位于 `app-kit::render_pipeline`：

```rust
pub enum RenderMode {
    Realtime,
    Offline,
}
```

默认值是 `Realtime`。Truvis 还支持 `TRUVIS_RENDER_MODE=Offline` 作为启动期验证入口；它只设置初始模式，运行中仍可通过 ImGui 切换。

`OfflinePipeline` 是独立 pipeline owner，负责创建 pass、目标图像、命令缓冲和 RenderGraph 贡献方法：

```text
OfflinePipeline
  settings: OfflinePipelineSettings
  accum_state: OfflineAccumState
  targets: OfflineTargets
  offline_rt_pass
  accum_pass
  sdr_pass
  resolve_pass
```

`OfflineTargets` 保存离线管线自己的 GPU image：

```text
OfflineTargets
  single_frame_image: per-FIF ImageTarget
  accum_image: unique ImageTarget
  render_target: per-FIF ImageTarget
```

`OfflineAccumState` 独立记录离线累计历史：

- `sample_count`：已经累计进 `accum_image` 的有效样本数。
- `signature`：相机、scene 版本和会改变离线 radiance 的设置签名。
- `sample_jitter_px`：从下一离线 sample index 推导的 Halton jitter，和 runtime/DLSS temporal jitter 完全隔离。
- reset 触发条件：camera/view 变化、resize、TLAS scene revision、emissive light/material revision、analytic light version、sky distribution version、离线 debug/sky/NEE 设置变化。
- 不 reset 条件：曝光、ACES strength、white point 等 tone mapping 参数变化。

`RenderSceneView` 额外暴露 `RenderSceneAccumSignature`，只包含版本号快照，不暴露 `GpuScene` owner：

```text
RenderSceneAccumSignature
  tlas_revision
  emissive_light_version
  analytic_light_version
  sky_distribution_version
```

`OfflinePipelineSettings` 当前只包含离线已使用的设置：

- 每帧 RT dispatch 数，范围固定为 1-8；它只改变累计推进速度，不进入 reset 签名。
- debug channel（排除 ReSTIR-only channel）。
- sky sampling mode。
- sky brightness。
- emissive NEE / analytic NEE 开关。
- tone mapping / exposure 参数。

它不包含 DLSS、RR、ReSTIR 或 denoise 设置。

## 与实时管线边界

实时和离线共享的是 shader 级光照与材质评估工具，不共享 temporal reconstruction 状态。

共享内容：

- `RenderSceneView`、TLAS、scene buffer、material / texture / light / sky GPU 数据。
- realtime RT 中已整理的 NEE、MIS、candidate、visibility、material 和 path-state helper。
- 通用后处理 pass，例如不含 DLSS 依赖的 SDR / tone mapping pass。
- `AccumPass` 的基础公式和 RenderGraph image 声明模式，但 sample count 来源是 `OfflineAccumState`。

不共享内容：

- DLSS SR / RR feature resource、input image、output image 和 history。
- ReSTIR reservoir、surface key、history signature 和 mode。
- realtime GBuffer / motion vector。
- runtime `ViewAccumState`。

ImGui 模式切换属于 app-kit 共享能力。离线模式下，DLSS 与 ReSTIR 控件保留在 Controls 面板中，但 disabled，并显示它们只影响实时模式。

## 已完成实现步骤

### 第一批：文档与类型骨架

- 新增本 brain-storm 文档，并在 `docs/brain-storm.md` 登记。
- 在 app-kit 渲染管线层新增共享 `RenderMode { Realtime, Offline }`，默认 `Realtime`。
- 明确 `OfflinePipeline`、`OfflineTargets`、`OfflineAccumState` 和 `OfflinePipelineSettings` 的职责边界。

### 第二批：离线资源与 graph

- 新增 `OfflineTargets`，按现有 app-owned target 模式创建 per-FIF `single_frame_image`、FIF 唯一 `accum_image` 和 per-FIF `render_target`。
- 给离线三类 image 注册 UAV / SRV，确保 compute 写入、present / GUI 读取和 bindless 调试路径可用。
- 新增 `OfflineAccumState`，由离线管线维护 sample count 和 reset 签名。
- 调整 `AccumPass` 的 RenderGraph adapter，让 sample count 由调用方传入。
- 离线 compute graph 完成 `single_frame_image -> accum_image -> render_target`。
- Truvis 按 `RenderMode` 选择 realtime/offline compute graph 与 present input。
- `OfflinePipelineSettings.ray_dispatch_count` 控制每帧添加多少组 `offline ray tracing -> accum` pass；调节该值不重置
  `accum_image`，只改变后续 sample 推进速度。

### 第三批：离线 RT pass

- 新增 `OfflineRtPass` 和 `offline_rt` shader entry。
- `offline_rt` pass API 已纳入 `api/mod.slangi`，Rust 侧使用生成的 `gpu::offline_rt::PushConstants` 和
  `gpu::OFFLINE_RT_SET_NUM`，不维护手写镜像 ABI。
- offline raygen 不写 DLSS / ReSTIR / GBuffer / motion vector 资源。
- 复用 realtime RT 的 NEE、MIS、material、path-state 和 sky helper。
- 实时 payload include 加 include guard，offline payload 通过局部 namespace remap 让 realtime helper 访问离线的 TLAS / output ABI。

### 第四批：UI、reset 与验证

- Controls 面板增加 `Render Mode` combo。
- Controls 离线面板增加 `RT Dispatches / Frame`，范围为 1-8，默认 1。
- 离线模式下禁用 DLSS SR、DLSS RR 和 ReSTIR DI 控件，并显示实时专用说明。
- 切出离线模式不销毁 `accum_image`；切回离线时签名未变则继续累计。
- Debug image viewer 注册 offline `single_frame_image`、`accum_image` 和 `render_target`。
- `TRUVIS_RENDER_MODE=Offline` 可用于启动期直接覆盖离线 graph 验证。

## 后续扩展点

- 在 `OfflinePipelineSettings` 中加入最大路径深度、Russian roulette 开关和起始深度。
- 如果需要更严格 ground truth 统计，可把 online mean 扩展为 sum/count、variance 或 adaptive sampling 格式。
- 如果未来离线 debug 需要 primary surface 信息，应新增离线自有 debug target，不复用 realtime GBuffer。
- 自动化 UI 验证需要 Windows Computer Use 工具可用；当前代码路径已经支持 ImGui 切换，但本次自动点击验证受工具连接问题限制。

## 验证记录

已执行：

- `cargo check`：通过。
- `just shader-force`：通过，`offline_rt` 与 `post/image_clear` shader entry 全部编译成功，Rust shader binding 更新成功。
- `cargo build --bin truvis-app`：通过。
- `TRUVIS_RENDER_MODE=Offline` + Vulkan validation 运行 Truvis：日志确认执行 `offline-ray-tracing -> offline-accum -> offline-hdr-to-sdr`。`Offline accumulation sample_count` 在当前二进制验证中推进到 512。

validation 结果：

- 未发现离线 graph 相关 VUID / validation error。
- 日志中只有 validation 配置性能警告 `VALIDATION-SETTINGS`，以及 Streamline wrapper 移除重复 buffer-device-address 扩展的提示。

仍需人工或可用 GUI 自动化确认：

- 在运行中通过 ImGui 手动切换 `Realtime / Offline`。
- 观察 Offline Samples 文案递增。
- 移动相机、resize、切换 sky / light / material 后确认离线累计 reset；只调 exposure / tone mapping 时确认不 reset。
