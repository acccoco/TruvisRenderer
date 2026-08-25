# ARCHITECTURE.md

本文是项目当前架构的唯一入口与导航页，只保留最高优先级约束和详细文档入口。分层、生命周期、数据流、
状态与资源契约由 `docs/summaries/` 中的主题文档承载；具体 crate 入口、局部职责和运行命令由对应模块 README 承载。

## 推荐阅读顺序

先建立跨层心智模型：

1. [`layering-and-dependency-boundaries.md`](summaries/layering-and-dependency-boundaries.md)：总体分层、依赖方向与 app/engine 边界。
2. [`frame-lifecycle.md`](summaries/frame-lifecycle.md)：启动、统一帧执行器，以及 Runtime/App/Plugin phase 顺序。
3. [`runtime-app-plugin-boundaries.md`](summaries/runtime-app-plugin-boundaries.md)：状态所有权、Ctx 裁剪、`RenderAppRunner` 与 Plugin 模型。
4. [`threading-and-resource-lifecycle.md`](summaries/threading-and-resource-lifecycle.md)：线程、GPU 同步和资源创建/重建/销毁契约。

再按任务选择专题：

| 主题 | 当前实现事实入口 |
| --- | --- |
| CPU Scene、asset identity、GPU scene 与 prepare | [`scene-data-lifecycle.md`](summaries/scene-data-lifecycle.md) |
| RenderGraph、pass 顺序、image 状态与提交 | [`render-graph-and-data-flow.md`](summaries/render-graph-and-data-flow.md) |
| Runtime/App/Plugin 配置与派生状态 | [`render-configuration-system.md`](summaries/render-configuration-system.md) |
| Realtime RT、NEE、MIS、ReSTIR 与 SHARC | [`realtime-rt-raytracing-flow.md`](summaries/realtime-rt-raytracing-flow.md) |
| Tauri Web Editor、协议、背压与一致性 | [`editor-subsystem.md`](summaries/editor-subsystem.md) |

模块入口：

- [`engine/README.md`](../engine/README.md)：Engine 目录与 crate 导航。
- [`truvis-render-thread/README.md`](../engine/platform/truvis-render-thread/README.md)：窗口 backend 无关的渲染线程生命周期。
- [`truvis-winit-host/README.md`](../engine/platform/truvis-winit-host/README.md)：standalone 与 embedded winit 窗口宿主。
- [`app/README.md`](../app/README.md)：App 域、公共组件、主体 App 与 samples。
- [`app/truvis/README.md`](../app/truvis/README.md)：主体 App 的状态 owner 与 pipeline 编排。
- [`app/editor/README.md`](../app/editor/README.md)：Editor 构建、协议源码和运行参数。
- [`docs/brain-storm/README.md`](brain-storm/README.md)：仍未进入主线实现的活跃设计方向。

## 全局架构约束

- 项目保持无环依赖：上层可以依赖下层，下层不反向依赖上层业务；同层 crate 默认不互相依赖，
  除非 summaries 中明确记录。
- 平台层拆分为 winit 窗口宿主与 backend-independent 渲染线程宿主，依赖方向为
  `truvis-winit-host -> truvis-render-thread -> truvis-app-frame`。主体 Tauri App、embedded child HWND 与独立
  samples 都由窗口 owner 持有统一的 `RenderThread` handle，并在其 OS 渲染线程内创建具体 App、进入唯一
  `RenderAppRunner::run`；所有 Vulkan 对象只在该线程创建、使用和销毁。
- `RenderRuntime` 拥有 `Gfx`、`World`、GPU resource/binding/timing owner、runtime render state、
  `RenderWorld`、present、command 和同步资源；App/Plugin 只通过 phase-appropriate Ctx 使用窄能力。
- App state 持有 GUI、camera/input、overlay 和具体 pipeline，并显式决定 RenderGraph pass 顺序；
  标准 `Plugin` trait 只承载通用生命周期，特有能力由具体类型接口暴露。
- CPU scene 语义只由 `World`/`SceneStore` 拥有；GPU scene 是 prepare 后的派生状态，render pass 只读取
  `RenderSceneView`，不访问 CPU owner 或 render-side manager。
- GPU 资源以显式 owner 为生命周期边界；Vulkan/VMA/WSI wrapper 通过显式 `destroy` 路径释放，
  `Drop` 不调用底层 release API。

## 文档职责

- `docs/ARCHITECTURE.md`：当前架构入口、阅读顺序和最高优先级约束。
- `docs/summaries/`：当前实现事实，记录跨模块分层、生命周期、状态所有权、数据流、线程与资源契约。
- 模块内 `README.md`：模块为何存在、拥有什么和不拥有什么、依赖方向、局部生命周期与常见操作入口。
- `docs/brain-storm/`：仍有明确工程价值但尚未实现的设计方向；完成后提炼事实并删除，不建立归档机制。
- 代码注释、类型、断言和测试：局部实现意图与可执行约束，不在长期文档中复制完整代码结构。
