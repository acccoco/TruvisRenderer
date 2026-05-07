# render-pipeline-plugin Specification

## Purpose
TBD - created by archiving change plugin-system-baseapp. Update Purpose after archive.
## Requirements
### Requirement: 渲染管线 SHALL 作为具体 Plugin 被 App 持有

Triangle、ShaderToy、RT Cornell/Sponza 等特定渲染能力 SHALL 从单一 `FramePlugin` demo 实现中拆出为具体 Plugin struct。App SHALL 直接持有这些具体类型，并在 `FrameAppHooks` 中编排它们。

#### Scenario: App 持有渲染管线 Plugin

- **WHEN** 检查迁移后的 demo App
- **THEN** App struct SHALL 持有对应的渲染管线 Plugin 字段
- **AND** render loop SHALL NOT 直接持有或调用这些渲染管线 Plugin

### Requirement: 渲染管线 Plugin SHALL 管理自身 GPU 资源生命周期

渲染管线 Plugin SHALL 通过 `Plugin::init` 创建 pipeline、pass、shader binding、尺寸相关 GPU resource 等自身资源，通过 `Plugin::on_resize` 重建尺寸相关资源，通过 `Plugin::shutdown` 释放自身资源。

#### Scenario: Pipeline 初始化通过 Plugin lifecycle 执行

- **WHEN** App 初始化插件集合
- **THEN** 每个渲染管线 Plugin SHALL 通过 `Plugin::init(&mut PluginInitCtx)` 获取创建资源所需上下文

#### Scenario: Pipeline resize 通过 Plugin lifecycle 执行

- **WHEN** swapchain 或 frame-sized resource 发生重建
- **THEN** App SHALL 将 `PluginResizeCtx` 传给需要 resize 的渲染管线 Plugin

### Requirement: 渲染管线特有能力 SHALL 通过具体方法暴露

渲染管线 Plugin 的 render graph 贡献 SHALL 通过具体类型方法暴露，而非加入统一 `Plugin` trait。推荐方法形态为 `contribute_passes(&self, graph: &mut RenderGraphBuilder, ctx: &PluginRenderCtx)`；若该管线需要在 render hook 中更新 per-frame 内部 GPU buffer，方法 MAY 使用 `&mut self`。

#### Scenario: App 组合多个渲染管线 Plugin

- **WHEN** App 在 `FrameAppHooks::render` 中构建 RenderGraph
- **THEN** App SHALL 按自身需要调用各渲染管线 Plugin 的 `contribute_passes` 或等价具体方法
- **AND** App SHALL 决定 pass 顺序、graph 拓扑和最终 execute/submit 时机

### Requirement: Demo SHALL 不再把整 App 当作唯一 FramePlugin

四个 demo SHALL 迁移为 `FrameApp` + App 持有的具体 Plugin 组合。原 `FramePlugin::render` 中直接构建 graph 的逻辑 SHALL 移入 App 的 `FrameAppHooks::render`，具体 pipeline/pass 创建与 graph 贡献逻辑 SHALL 归属对应 Plugin。

#### Scenario: Demo render path 由 App 编排

- **WHEN** 运行 `triangle` / `shader-toy` / `rt-cornell` / `rt-sponza`
- **THEN** 渲染线程 SHALL 通过 `FrameApp::run_frame` 推进
- **AND** 每个 demo 的 App SHALL 在 render hook 中编排 `GuiPlugin` 和对应渲染管线 Plugin
