# Brain Storm 文档索引与当前状态

本目录保存架构发散、设计草案和阶段性诊断。当前实现事实以仓库根目录的
[`ARCHITECTURE.md`](../ARCHITECTURE.md) 为准；这里的文档只记录仍有参考价值的
设计方向、已归档历史和未完成问题。

## 当前代码快照（2026-05-23）

已经落地、后续不应再作为待办重复推进的内容：

- winit 主线程与渲染线程已分离；主线程只负责事件循环、窗口生命周期和跨线程信号。
- `RenderAppShell` 是平台无关的帧骨架，具体 App 通过 `RenderAppHooks` 接入。
- `Plugin` trait 已提供 init / input / update / resize / shutdown 生命周期，具体 App 通过字段显式组合插件能力。
- `RenderRuntime` 持有 `World + GpuStore + GpuScene + present/cmd/sync`，并通过 typed lifecycle Ctx 暴露窄能力。
- `World` 是 CPU 侧聚合层，只持有 `SceneManager + AssetHub`。
- `AssetHub` 已收敛为内容资产身份、CPU 加载状态和加载事件来源，不再创建 GPU image/view 或注册 bindless。
- texture / mesh / material / instance 的 GPU 可见状态由 render-side manager / bridge 管理。
- `GpuScene` 与 `RenderData` 已成为 `truvis-render-runtime` 私有 scene 翻译层，pass 只通过 `RenderSceneView` 读取。
- `truvis-render-graph` 与 app 层 `app-render-passes` 不再依赖 `truvis-world` / `truvis-asset`。
- `truvis-render-foundation` 是当前渲染基础层名称，不再使用旧 `render-interface` 命名。

仍然存在、brain-storm 文档中的讨论还有效的边界问题：

- `RenderRuntime::prepare()` 仍把 scene extract、GPU scene upload、per-frame uniform 和 descriptor 更新串在一个 prepare 入口里；显式 `extract -> prepare -> render` phase 仍未成形。
- 当前只有 App 手写组合的插件列表，还没有 `PluginGroup`、依赖声明或拓扑校验。
- Camera / input / GUI / overlay 已移到 App 与 app-kit 侧，但仍不是声明式 builtin plugin。
- `ViewDesc` / `PreparedView` / `ViewStore` 尚未落地，当前仍是隐式 main view。
- `RenderPresent` 仍由 `RenderRuntime` 持有；更彻底的 `SurfaceRegistry` / 多窗口 / headless 边界仍是远期方向。
- `Gfx` 构造注入和进一步去全局访问仍未完成。
- DLSS / Streamline 接入需要先明确 Vulkan loader、C++ wrapper、RenderGraph opaque pass
  和 temporal resources 边界。
- 资产上传仍可继续探索 batched upload / staging thread，避免大量资源同帧 ready 时挤占 render thread。
- `app-kit` 仍承载 GUI、camera/input、overlay 与 pipeline glue，后续可继续拆分可复用能力。

## 活跃文档

- [`architecture-principles-and-open-issues.md`](brain-storm/architecture-principles-and-open-issues.md)：
  当前仍应遵守的架构原则、职责边界和开放问题。
- [`asset-scene-pipeline-status.md`](brain-storm/asset-scene-pipeline-status.md)：
  AssetHub、render-side manager/bridge、GpuScene 与 RenderSceneView 的当前状态。
- [`render-view-concept.md`](brain-storm/render-view-concept.md)：
  轻量 main view / prepared view 的引入方向。
- [`dlss-streamline-integration.md`](brain-storm/dlss-streamline-integration.md)：
  DLSS Super Resolution、Streamline Vulkan interposer、C++ wrapper 与 RenderGraph 接入边界。
- [`plugin-feature-evolution.md`](brain-storm/plugin-feature-evolution.md)：
  PluginGroup、pipeline feature、GUI / platform / event 分层的演进方向。
- [`threading-model-evolution.md`](brain-storm/threading-model-evolution.md)：
  当前线程拓扑与 asset upload / update thread 的后续取舍。
- [`naming-and-glossary.md`](brain-storm/naming-and-glossary.md)：
  当前术语、已完成命名决策和历史名称对照。

## 归档入口

历史草案和已落地方案移动到 [`archive/`](brain-storm/archive/README.md)。归档文档只作为
当时讨论记录阅读，不再代表当前主线 API 或模块路径。

归档原则：

- 已落地方案移动到 archive，避免在活跃目录里重复推进。
- 被后续重构覆盖的诊断只保留历史原文和归档状态说明。
- 多篇相似草案先提炼到活跃主题文档，再移动原文。
- 归档原文不作为当前事实来源；需要当前边界时先看 `ARCHITECTURE.md` 和本索引。

## 维护规则

- 新增 brain-storm 文档时，先确认是否能合并进现有活跃主题。
- 活跃文档应优先记录当前状态、明确边界和下一步方向，不保留大段历史论证。
- 旧路线落地后，更新本索引和对应活跃主题，再把源草案移动到 archive。
- 不把本目录当作唯一事实来源；当前实现边界仍以 `ARCHITECTURE.md` 和代码为准。
