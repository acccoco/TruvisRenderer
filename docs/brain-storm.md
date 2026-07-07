# Brain Storm 文档索引

本目录只记录尚未进入主线实现、但仍有明确工程价值的设计方向和方案评估。当前实现事实以
[`docs/ARCHITECTURE.md`](ARCHITECTURE.md)、[`docs/summaries/`](summaries/) 和模块 README 为准。

这里的文档不是事实摘要，也不是历史归档。一个方向完成后，应把仍需保留的事实提炼到 `docs/summaries/`
或模块 README，再从本目录移除；历史讨论通过 Git 历史追溯。

## 活跃方向

- [`runtime-boundary-evolution.md`](brain-storm/runtime-boundary-evolution.md)：
  runtime phase、main view、surface/headless 和 `Gfx` owner 边界的后续收敛。
- [`render-graph-evolution.md`](brain-storm/render-graph-evolution.md)：
  RenderGraph resource model、跨 graph 状态校验、adapter helper 与 FrameGraph 方向。
- [`plugin-and-app-kit-evolution.md`](brain-storm/plugin-and-app-kit-evolution.md)：
  PluginGroup、PipelineFeature、app-kit builtin feature 和分层事件模型。
- [`asset-upload-and-scene-evolution.md`](brain-storm/asset-upload-and-scene-evolution.md)：
  asset upload、热重载、跨场景卸载和 scene invalidation 的后续能力。
- [`realtime-lighting-evolution.md`](brain-storm/realtime-lighting-evolution.md)：
  realtime RT light-class 策略、ReSTIR 稳定性、SHARC 历史控制和间接光复用评估。
- [`dlss-quality-and-cleanup.md`](brain-storm/dlss-quality-and-cleanup.md)：
  DLSS 画质验证、specular motion vector 后续质量、运行时降级和旧 pass 清理。
- [`offline-rendering-evolution.md`](brain-storm/offline-rendering-evolution.md)：
  离线渲染设置、累计统计、专用 debug target 和交互验证能力。

## 维护规则

- 每个文档必须对应一个可独立推进的工程方向，避免把多个 owner 或多个阶段无边界地混在一起。
- 文档结构统一为：目标、当前基线、待推进内容、边界与非目标、完成标准。
- 不复述已进入 summaries 的实现事实；只用少量链接说明阅读入口。
- 不建立归档目录；失去当前价值的讨论直接删除。
