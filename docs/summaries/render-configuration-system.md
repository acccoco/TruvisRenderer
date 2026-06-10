# 渲染配置体系

> 状态：当前实现事实总结。本文只描述和渲染相关的配置、派生状态、调试选项与非配置状态边界。

## 总体分层

当前配置体系按所有权与生命周期分成四类：

| 类别 | 代表类型 | 所在层 | 是否用户可调 | 主要职责 |
|------|----------|--------|--------------|----------|
| runtime 全局渲染选项 | `RenderOptions` | `truvis-render-runtime` | 是 | 保存会影响 runtime 渲染状态的少量全局选项 |
| runtime 派生帧状态 | `FrameRenderState` | `truvis-render-runtime` | 否 | 保存当前 main view 的格式、render extent 与 output extent |
| main view temporal 状态 | `ViewAccumState`、`DlssSrState` | `truvis-render-runtime` | 否 | 追踪历史是否可复用，以及 DLSS common constants / reset |
| app / pipeline 局部设置 | `RtPipelineSettings`、`DenoiseAccumSettings` | app 层 | 取决于 app | 保存具体 pipeline 自己理解的调试或实验参数 |

这次整理后的核心原则是：`truvis-render-runtime` 持有跨 pipeline 的渲染契约和 runtime 派生状态，
包括 `RenderOptions`、`FrameRenderState`、`ViewAccumState` 与 `DlssSrState` 等明确 owner；具体 RT pass 的 debug channel、SDR
tone mapping、legacy denoise 参数和实验性 IC 开关不再伪装成 engine 全局配置。

## RenderOptions

`RenderOptions` 位于 `truvis-render-runtime::state::render_options`，是用户或调试 UI 可以修改的 runtime 全局渲染选项。

当前包含：

| 字段 | 含义 | 修改后如何生效 |
|------|------|----------------|
| `dlss_sr_mode: DlssSrMode` | DLSS SR / DLAA 模式 | `RenderRuntime::sync_render_options_frame_state` 解析该 mode，必要时更新 `FrameRenderState`、触发 app-owned target rebuild，并重置 DLSS history |
| `dlss_rr_enabled: bool` | 是否用 DLSS RR 替代普通 SR evaluate | 非 `Off` mode 下生效；切换时 runtime 等待 GPU idle，释放旧 DLSS feature resources，并重置 DLSS history |

`TRUVIS_DLSS_SR_MODE` 和 `TRUVIS_DLSS_RR` 会在 runtime 初始化时作为启动默认值读取。运行中仍以 ImGui overlay 修改 `RenderOptions`，再由 shell 在 `prepare` 前调用同步入口使其生效。

不放入 `RenderOptions` 的内容：

- RT debug channel：只属于 RT pipeline 的 shader 调试输出。
- SDR tone mapping：只属于 RT pipeline 的最终显示映射，不影响 runtime target 尺寸或 DLSS history。
- denoise 参数：当前主 RT 流程已旁路传统 denoise/accum pass；保留 pass 时只能作为 pass-local 实验设置。
- IC 开关：Irradiance Cache 仍是 shader 实验路径，主流程 push constant 固定为关闭。

## DlssSrMode 与 DlssSrState

`DlssSrMode` 位于 `truvis-render-runtime::state::dlss_sr`，和 `DlssSrState` 放在同一个 DLSS SR 语义边界内。

| mode | render extent 行为 | 执行行为 |
|------|--------------------|----------|
| `Off` | `render_extent == output_extent` | native 路径，不调用 DLSS SR pass |
| `Dlaa` | `render_extent == output_extent` | 调用 DLSS feature 做抗锯齿，不做 upscale |
| `Quality` / `Balanced` / `Performance` / `UltraPerformance` | 通过 Streamline optimal settings 派生低分辨率 `render_extent` | RT/GBuffer/DLSS input 用低分辨率渲染，DLSS output 回到 `output_extent` |

`DlssSrState` 不是配置。它保存每帧 evaluate 所需的 common constants、previous view、temporal jitter sequence 和 reset 标记。DLSS SR / DLAA / RR 启用时，它按 Halton(2,3) 生成 pixel-space frame-wide jitter；DLSS Off 时 jitter 为 0 且不推进序列。窗口尺寸变化、render extent 变化、mode 切换等会调用 `request_reset`，让下一次 DLSS evaluate 丢弃旧 history，并把 jitter sequence 重置到固定起点。

## FrameRenderState

`FrameRenderState` 位于 `truvis-render-runtime::state::frame_state`，是 runtime 根据窗口、present、DLSS SR mode 和设备能力推导出的当前 main view 帧状态。

| 字段 | 来源 | 用途 |
|------|------|------|
| `hdr_color_format` | runtime 默认 HDR 中间格式 | app-owned RT target、main view target、DLSS input/output 等 HDR 图像格式契约 |
| `depth_format` | runtime 按设备能力选择 | depth attachment 与 depth image view 创建 |
| `render_extent` | swapchain extent 与 DLSS mode 派生 | RT、GBuffer、motion vector、DLSS input 等内部渲染尺寸 |
| `output_extent` | swapchain / present extent | GUI、present、main-view output 与 DLSS output 尺寸 |

`FrameRenderState` 不由用户直接改。App / Plugin 在 init、resize 或 `sync_render_options_frame_state` 返回 resize ctx 时读取它，用来重建自己持有的窗口尺寸 target。

## ViewAccumState

`ViewAccumState` 位于 `truvis-render-runtime::state::view_accum`，表达当前 main view 的 temporal accumulation 状态。

它追踪上一帧的 `RenderViewAccumSignature` 和连续稳定帧数。只要相机、关键 view 参数、sky 绑定或尺寸状态导致历史不再匹配当前 view，就会 reset。

它不是配置，也不决定 pass 是否启用累积。当前 RT 主流程不再做 progressive accumulation，但该状态仍作为 main view temporal state 保留，供保留的传统 pass 或调试信息读取。

## RtPipelineSettings

`RtPipelineSettings` 位于 app 层 `app-kit::render_pipeline::rt_render_graph`，由 `RtPipeline` 持有。

当前包含：

| 字段 | 含义 |
|------|------|
| `debug_channel: RtDebugChannel` | 主 RT shader / SDR pass 使用的调试输出通道 |
| `tone_mapping: SdrToneMappingSettings` | SDR 输出路径使用的手动曝光和 ACES fitted tone mapping 参数 |

RT debug channel 与 tone mapping 只在 Truvis / Cornell 等 RT app 的 overlay 中显示；Hello Triangle / ShaderToy 只显示 DLSS SR mode，不暴露 RT 调试或 tone mapping 参数。

`RtDebugChannel` 使用 enum 表达当前主 RT 流程支持的通道：final、normal、base color、NEE HDRI、emission、BRDF HDRI、NEE bounce 0/1 和 IC debug。旧的 magic number “not accum” 通道不再通过 UI 暴露。

`SdrToneMappingSettings` 只作用于 `hdr-to-sdr` pass 的 Final 通道。当前使用实时渲染常用的 ACES fitted approximation，并提供 `Exposure EV`、`ACES Strength` 与 `White Point` 三个 ImGui 调节项；它不是完整 ACES / OCIO / HDR10 display transform，也不做自动曝光或参数持久化。

## Runtime Defaults

`DefaultRenderRuntimeSettings` 已从 foundation 移到 `truvis-render-runtime` 内部模块。它只描述 runtime 初始化策略：

- 默认 surface format。
- 默认 present mode。
- depth format 候选顺序。

这些默认值不是公共配置契约。App 不应该依赖 runtime 必定选择某个 depth format 或 present fallback。

## 不是配置的内容

以下内容虽然会影响渲染，但不属于配置项：

| 内容 | 为什么不是配置 |
|------|----------------|
| `FrameLabel` | 它只是 FIF A/B/C slot 索引，用于选择当前帧 command buffer、descriptor、per-frame image |
| per-frame UBO | 它是每帧由 `RenderView`、scene、timer 和 `FrameRenderState` 写出的 GPU 数据快照 |
| RenderGraph image state | 它描述单帧 graph 内的读写状态与同步计划，不是长期配置 |
| resource handles | 它们是 manager-owned GPU resource 的索引或句柄，不表达策略 |
| `PresentView` / swapchain image wrapper | 它们是 WSI 资源访问视图，不是渲染质量或 pass 行为配置 |

## DLSS Mode 生效流程

DLSS mode 的变化在一帧中按固定路径生效：

```text
Overlay 修改 RenderOptions.dlss_sr_mode / dlss_rr_enabled
  -> RenderAppShell 调用 RenderRuntime::sync_render_options_frame_state
  -> runtime 用 output extent + mode 查询 Streamline optimal settings
  -> 派生 FrameRenderState.render_extent / output_extent
  -> 如尺寸变化，返回 RenderRuntimeResizeCtx
  -> App / Plugin 重建 RT target、GBuffer、DLSS input/output、main view target
  -> 如 SR/RR feature 分支变化，runtime 等待 GPU idle 并释放旧 feature resources
  -> runtime 重置 ViewAccumState 与 DlssSrState history
  -> 下一帧 prepare/render graph 使用新的 render/output extent 和 active DLSS feature
```

如果 Streamline 查询失败或返回非法尺寸，runtime 会把 `RenderOptions.dlss_sr_mode` 降级为 `Off`，并回到 native extent，保证 app 仍能继续渲染。

从 DLSS SR / DLAA / RR 切回 `Off`，或在 SR 与 RR 之间切换时，runtime 会先等待 GPU idle，再调用对应 feature 的 `slFreeResources` 释放 Streamline viewport 0 的内部资源；这是因为这些内部 image / buffer 可能仍被上一帧 DLSS evaluate 录制的 command buffer 引用。
