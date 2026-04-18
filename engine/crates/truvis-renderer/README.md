# truvis-renderer

渲染整合层，负责帧生命周期驱动与核心子系统编排。

## 主要职责

- 管理 begin/update/render/present 的帧阶段推进
- 聚合 RenderContext 所需子系统
- 与 swapchain / command 提交 / 同步机制对接

## 与其他模块关系

- 上承 `truvis-app`（应用逻辑）
- 下接 `truvis-gfx`、`truvis-render-interface`、`truvis-render-graph`
