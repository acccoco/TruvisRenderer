# Offline Rendering 演进

## 目标

在现有离线渲染主线之上扩展更适合 reference / ground truth 的控制和统计能力。目标是提升可验证性和可诊断性，
而不是复述离线管线资源拓扑。

## 当前基线

当前 realtime/offline graph 关系、离线累计资源、公共 path tracing 设置和 present 叠加顺序见
[`../summaries/render-graph-and-data-flow.md`](../summaries/render-graph-and-data-flow.md) 和
[`../summaries/render-configuration-system.md`](../summaries/render-configuration-system.md)。

## 待推进内容

- Path 设置：加入最大路径深度、Russian roulette 开关或起始深度等离线专属调参，并定义哪些设置会 reset 累计。
- 累计统计：评估从 online mean 扩展为 sum/count、variance 或 adaptive sampling 的格式和 reset 签名。
- 离线 debug target：如果需要 primary surface、sample variance 或 per-bounce 诊断，新增离线自有 target，
  不复用 realtime GBuffer 或 motion vector。
- 交互验证：建立模式切换、sample count 推进、相机移动、resize、sky/light/material edit 和 tone mapping 调节的 GUI 验证流程。
- 输出策略评估：如需保存 reference 图，再定义文件输出、色彩空间和命名策略；不默认加入主流程。

## 边界与非目标

- 不把 DLSS、RR、ReSTIR、SHARC 或 realtime temporal state 接入离线累计。
- 不在第一轮追求完整 production renderer 的采样器、AOV 或 denoise 管线。
- 不让离线 debug target 污染 realtime pipeline resource model。
- 不把已进入 summaries 的离线主流程复制到本文件。

## 完成标准

- 离线专属设置的 reset 规则清楚，并能通过 UI 或日志观察。
- 累计统计格式能支持后续 variance / adaptive sampling，而不破坏当前 present 路径。
- 离线 debug target 和 realtime debug image 的 owner、尺寸和生命周期可区分。
- GUI 验证流程能覆盖模式切换与关键 reset / non-reset 场景。
