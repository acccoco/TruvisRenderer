# Brain Storm 文档索引与当前状态

本目录保存架构发散、设计草案和阶段性诊断。这里的文档允许互相矛盾，
因为它们记录的是不同时间点、不同演进方向的思考过程。

当前代码事实以仓库根目录的 [`ARCHITECTURE.md`](../ARCHITECTURE.md) 为准；
本文件只负责把 brain-storm 文档按主题、状态和后续价值整理出来。

## 1. 当前代码快照（2026-04-23）

已经落地、后续不应再作为待办重复推进的内容：

- `RenderApp` / `OuterApp` 主路径已经演进为 `FrameRuntime` / `AppPlugin`。
- `truvis-winit-app` 已经只做 winit 事件循环、窗口生命周期和渲染线程入口。
- `FrameRuntime` 已经成为帧编排入口，外部通过 `push_input_event` / `time_to_render` / `run_frame` / `destroy` 驱动。
- `AppPlugin` typed contexts 已经拆到 `truvis-app-api`。
- `World` 与 `RenderWorld` 已经物理拆分，`Renderer` 现在持有 `World + RenderWorld`，旧的 `RenderContext` 已从主线退场。
- `truvis-render-graph` 已经不再依赖 `scene` / `asset`。
- `truvis-gui-backend` 保持为 Vulkan ImGui 后端，RenderGraph 适配 `GuiRgPass` 仍在 `truvis-app`。
- 通用 pass 已拆到 `truvis-render-passes`。

仍然存在、文档中的讨论还有效的边界问题：

- `truvis-scene` 仍会通过 `SceneManager::prepare_render_data()` 接触 `AssetHub`、`BindlessManager` 和 `RenderData`。
- `truvis-asset` 仍在 `AssetHub::new/update/destroy` 中接收 `BindlessManager` 并注册 / 注销 SRV。
- `Renderer::before_render()` 仍把 scene extract、GPU upload、per-frame uniform 和 descriptor 更新揉在一起，`phase_extract` 还没有显式化。
- `FrameRuntime` 仍硬编码 `CameraController`、`InputManager`、`GuiHost` 和 overlays；多 plugin / builtin plugin 体系还未落地。
- `AppPlugin` 仍是单插件槽位，不支持 `PluginGroup`、依赖声明或有序插件列表。
- `truvis-app-api` 仍暴露 `truvis-renderer` 中的 `Camera` / `RenderPresent` 等具体 backend 类型。
- `RenderPresent` 仍同时持有 swapchain/present 资源和 `GuiBackend`。
- `truvis-render-interface` 名字仍偏“接口”，但内容是 concrete render core：manager、`RenderWorld`、`GpuScene`、`RenderData`。
- `truvis-app` 仍同时承担 demo、pipeline glue、RenderGraph 适配和过渡期依赖。
- `truvis-render-passes` 仍依赖 `truvis-world`，说明 pass 层还可触达 CPU World。
- `Gfx::get()` 全局单例仍存在。

尚未实现、仍属于演进方向的内容：

- `SceneBridge` / `SceneSnapshot`，把 CPU scene 语义和 GPU render data 构建拆开。
- `AssetReadyEvent` / `AssetTextureEvent` 和 `TextureBindingCache`，把 asset ready 与 bindless 注册拆开。
- `truvis-render-interface -> truvis-render-core` 或更彻底的 render core / render scene 拆分。
- `ViewDesc` / `PreparedView` / `ViewStore`，把隐式 main view 显式化。
- 显式 `extract -> prepare -> render` phase。
- 多 plugin / builtin plugin / plugin dependency declaration。
- `PipelineFeature` / `PipelineManager`，让 raster / RT / Shadertoy 以策略层选择。
- `SurfaceRegistry`，把 swapchain / present 从 `Renderer` / `RenderPresent` 中进一步分离。
- `Gfx` 构造注入，减少全局单例访问。

## 2. 文档分组

### 当前诊断与近期演进

这些文档最贴近当前代码，可作为近期架构判断的主要参考：

- [`2026-04-23-structure-responsibility-open-source-comparison.md`](brain-storm/2026-04-23-structure-responsibility-open-source-comparison.md)：
  当前职责混乱点与开源渲染器对照。建议优先看这篇。
- [`2026-04-23-assets-bindless-decoupling.md`](brain-storm/2026-04-23-assets-bindless-decoupling.md)：
  asset / bindless / material texture binding 的边界拆分。
- [`2026-04-23-asset-resource-naming.md`](brain-storm/2026-04-23-asset-resource-naming.md)：
  `truvis-asset` 与 `GfxResourceManager` 的命名语义。
- [`2026-04-23-render-view-concept.md`](brain-storm/2026-04-23-render-view-concept.md)：
  `ViewDesc` / `PreparedView` 的轻量引入路径。
- [`naming-renderworld-renderer-backend-app.md`](brain-storm/naming-renderworld-renderer-backend-app.md)：
  `RenderWorld` / `Renderer` / `FrameRuntime` / `AppPlugin` 的概念边界和重命名路线。
- [`plugin-pass-eventbus-evolution.md`](brain-storm/plugin-pass-eventbus-evolution.md)：
  多 plugin、pass 特性化和事件总线的中长期路线。

### 已落地或部分落地的历史记录

这些文档记录的许多问题已经被后续重构解决，应作为历史上下文阅读：

- [`clean-crate-dependencies.md`](brain-storm/clean-crate-dependencies.md)：
  已完成的 crate 依赖清理，render-graph / gui-backend 边界已按本文收敛。
- [`render-thread-isolation.md`](brain-storm/render-thread-isolation.md)：
  渲染线程剥离已落地；其中旧兼容入口描述与当前代码不完全一致。
- [`2026-04-22-architecture-evolution-gap-analysis.md`](brain-storm/2026-04-22-architecture-evolution-gap-analysis.md)：
  其中 `RenderContext -> World + RenderWorld` 已落地；显式 extract、多 plugin、builtin systems plugin 化仍未落地。
- [`render-app-layering-analysis.md`](brain-storm/render-app-layering-analysis.md)：
  `RenderApp` / `OuterApp` / `RenderContext` 的历史诊断。主问题已由 `FrameRuntime` / `AppPlugin` / `World + RenderWorld` 缓解，但 GUI、surface、extract、plugin 化方向仍有效。

### 理想模型与远期路线

这些文档不一定要求当前项目完全照做，更适合作为设计原则和对照标尺：

- [`ideal_layered_architecture.md`](brain-storm/ideal_layered_architecture.md)：
  Platform / AppShell / Main World / Render World 的理想模型。
- [`ideal-module-architecture.md`](brain-storm/ideal-module-architecture.md)：
  组件级归属分析，部分内容已被后续 `World + RenderWorld` 重构覆盖。
- [`plugin-imgui-winit-multi-pipeline-integration.md`](brain-storm/plugin-imgui-winit-multi-pipeline-integration.md)：
  ImGui、Winit、多管线以 plugin/feature 方式接入的设计方向。
- [`app-tick-system.md`](brain-storm/app-tick-system.md)：
  基于旧 `RenderApp` / `OuterApp` 的 tick 草案；可作为“Camera/Input 行为不应硬编码”的历史动机，而不是当前 API 草案。

## 3. 保留的分叉方向

以下矛盾不是错误，而是不同阶段的可选路线，应继续保留：

- `GuiRgPass` 归属：
  `clean-crate-dependencies.md` 选择放在 `truvis-app`；`ideal-module-architecture.md` 倾向放到 `renderer`；`render-app-layering-analysis.md` 又提出迁到 `gui-backend`。当前代码采用 `truvis-app`，后续可在 renderer/backend 或 render-feature 层重新评估。
- `BindlessManager` 归属：
  理想架构文档把它视作 Platform/device lifetime 资源；当前代码把它放在 `RenderWorld`。短期继续尊重当前结构，中长期可随 SurfaceRegistry/Gfx 注入一起重新划分。
- `Renderer` 命名：
  可以只重命名 `truvis-render-interface -> truvis-render-core`，也可以把 `Renderer` 改成 `RenderBackend`，还可以远期合并 `FrameRuntime + Renderer` 对外称 `Renderer`。这些是不同力度的命名演进。
- Plugin vs Tickable：
  `app-tick-system.md` 走轻量 `Tickable` 注册表；较新的 plugin 文档倾向固定 phase + builtin plugin。当前更适合优先考虑 builtin plugin，因为主线已经是 `AppPlugin`。
- Asset 短期 vs 长期边界：
  短期可以让 `AssetHub` 继续创建 GPU image/view，只移走 bindless 注册；长期目标是 `truvis-asset` 完全不依赖 `gfx` / `render-interface`。
- View 抽象力度：
  当前推荐轻量 `MainView` / `PreparedView` 起步，不提前实现 UE 风格重型 `ViewFamily`。

## 4. 建议优先级

结合当前代码，近期最值得做的是收紧已经暴露出混合职责的边界：

| 优先级 | 事项 | 主要收益 |
|---|---|---|
| P0 | `AssetHub` 不再接收 `BindlessManager`，引入 asset ready event / texture binding cache | asset 不再决定 shader-visible descriptor 策略 |
| P0 | `SceneManager::prepare_render_data()` 改为 `snapshot()` + render-side bridge | scene 回到 CPU 语义，extract 边界变清楚 |
| P1 | 显式拆出 `phase_extract` 与 `phase_prepare` | 把当前 `before_render()` 的混合职责拆开 |
| P1 | 收窄 `app-api` contexts，减少 `RenderPresent` / concrete backend 泄漏 | 插件契约更稳定 |
| P1 | 引入轻量 `ViewDesc` / `PreparedView` | 命名当前隐式 main view，为多视角和 per-view history 铺路 |
| P2 | 多 plugin / builtin plugin | Camera/Input/GUI/Overlay 从 runtime 硬编码走向可组合 |
| P2 | `truvis-render-interface -> truvis-render-core` 命名或内容收窄 | 名字与职责一致 |
| P3 | SurfaceRegistry / Gfx 注入 / app crate 拆分 | 更彻底的平台与应用层解耦 |

## 5. 维护规则

- 新增 brain-storm 文档时，先在本 README 中登记主题和状态。
- 如果文档描述的是历史结构，不要删除原文；在顶部加“维护状态”说明。
- 矛盾路线用“方案 A/B/C”或“短期/中期/长期”并列保留。
- 当某个方案落地后，在本 README 的“当前代码快照”里移动到“已经落地”，并给原文加状态提示。
- 不把本目录当作唯一事实来源；当前实现边界仍以 `ARCHITECTURE.md` 和代码为准。
