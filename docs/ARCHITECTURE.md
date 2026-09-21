# ARCHITECTURE.md

本文是项目当前架构入口，保留最高优先级约束和文档导航。当前顶层严格分为
`engine/`、`renderer/` 和 `app/`。

```text
app ──> renderer ──> engine
 │                      ▲
 └────> engine/platform ─┘
```

`renderer -> app` 和 `engine -> renderer` 都是禁止依赖。设计理念、不变量和取舍见
[`design/`](design/)；面向人的问题切片见 [`summary/`](summary/)。当前实现事实仍以代码、构建配置、测试、运行结果和模块 README 为准。

## 三层职责

- `engine/`：通用 Runtime、`RenderLoop`、`RenderThread`、RenderGraph、GameWorld、Vulkan RHI、窗口宿主及 shader 基础设施。Engine 不知道具体 Renderer 或 Tauri。
- `renderer/`：具体 Renderer、Subsystem、Pass、Shader、产品 overlay 和 transport-neutral typed ports。`TruvisRenderer`、Triangle 和 ShaderToy Renderer 都属于此层；Truvis 与 Cornell App 共用完整的 `TruvisRenderer`。
- `app/`：Tauri Editor 与 standalone sample 的启动壳，以及可注入 Renderer、运行在 RenderThread 上的产品业务 Client。Tauri `invoke/emit/AppHandle`、dialog、WebView、capabilities 和 timeout 只存在 App 主线程。

## 设计文档

1. [`layering-and-dependency-boundaries.md`](design/layering-and-dependency-boundaries.md)：顶层三层职责与依赖方向。
2. [`shader-abi-and-native-integration.md`](design/shader-abi-and-native-integration.md)：Shader ABI、Rust binding、C ABI 与 CXX owner。
3. [`runtime-phase-contract.md`](design/runtime-phase-contract.md)：Runtime、RenderLoop、Renderer、Subsystem 的阶段能力。
4. [`frame-execution-and-window-lifecycle.md`](design/frame-execution-and-window-lifecycle.md)：启动、帧循环、resize、present 和关闭。
5. [`threading-and-synchronization.md`](design/threading-and-synchronization.md)：线程 owner、消息边界和 GPU 同步。
6. [`resource-lifecycle-and-destruction.md`](design/resource-lifecycle-and-destruction.md)：GPU/CPU 资源创建、重建、退役和销毁。
7. [`scene-sync-and-render-world.md`](design/scene-sync-and-render-world.md)：CPU scene 到 GPU RenderWorld 的同步契约。
8. [`render-graph-and-data-flow.md`](design/render-graph-and-data-flow.md)：RenderGraph 资源访问、pass 顺序与提交。
9. [`render-configuration-and-temporal-state.md`](design/render-configuration-and-temporal-state.md)：配置、派生 frame state 与 temporal history。
10. [`realtime-raytracing-sampling.md`](design/realtime-raytracing-sampling.md)：Realtime RT、NEE、PDF/MIS 和 ReSTIR DI。
11. [`radiance-cache-and-debug.md`](design/radiance-cache-and-debug.md)：SHARC cache 与 debug 观测边界。
12. [`editor-boundary-and-consistency.md`](design/editor-boundary-and-consistency.md)：Editor 状态权威、typed ports、背压和恢复。

## 面向人的问题切片

以下文档只有在用户明确要求时更新；普通代码改动和设计维护不触发 summary 更新。

- [`render-runtime-phases.md`](summary/render-runtime-phases.md)：RenderRuntime 各阶段在做什么。
- [`render-runtime-object-hierarchy.md`](summary/render-runtime-object-hierarchy.md)：Runtime、World、Renderer、Subsystem 的对象层级和 owner。
- [`picking-flow.md`](summary/picking-flow.md)：从鼠标点选到 GPU 查询、selection、描边和 Editor 通知。

每篇 summary 都标明核对基准和“解释性快照”性质，不作为实现唯一事实来源。

## 模块入口

- [`engine/README.md`](../engine/README.md)：Engine 目录与 crate 导航。
- [`cxx/README.md`](../cxx/README.md)：native project、CXX module、构建工具与 Rust binding。
- [`renderer/README.md`](../renderer/README.md)：Renderer 层职责和组成。
- [`renderer/truvis-renderer/README.md`](../renderer/truvis-renderer/README.md)：`TruvisRenderer`、RendererClient 注入边界和 pass 编排。
- [`renderer/shader/README.md`](../renderer/shader/README.md)：Renderer shader package、ABI 和 binding owner。
- [`app/README.md`](../app/README.md)：Tauri 和 standalone 启动壳。
- [`app/editor/README.md`](../app/editor/README.md)：Web Editor 构建与 Tauri transport。
- [`truvis-render-thread/README.md`](../engine/e60-platform/truvis-render-thread/README.md)：渲染线程宿主。
- [`truvis-winit-host/README.md`](../engine/e60-platform/truvis-winit-host/README.md)：standalone 和 embedded winit 宿主。

## 全局约束

- `RenderRuntime` 拥有 `Gfx`、`GameWorld`、GPU resource/binding/timing owner、`RenderWorld`、present、command 和同步资源。
- Renderer 与 Subsystem 只通过当前 phase 的窄 Ctx 使用 Runtime 能力；App RenderThread Client 只通过 `RendererClient` 窄接口借用 CPU `GameWorld`。
- 具体 Renderer 拥有 camera/input、overlay、selection 和渲染子系统，并显式决定 RenderGraph pass 顺序。
- `SubsystemLifecycle` 只约束 init/resize/shutdown，controller 不实现该 trait。
- CPU scene 只由 `GameWorld`/`SceneStore` 权威持有；GPU scene 是 prepare 后的派生状态。
- Vulkan 对象只在 RenderThread 创建、使用和销毁。
- 窗口 owner 只有在 RenderThread 完成 Renderer/Runtime/Vulkan 回收后才销毁 child HWND。
- Shader owner 方向为 `engine <- renderer`，package manifest 是构建期 shader 依赖事实来源。
- workspace 根 `cxx/` 是唯一 native integration project；Rust binding 只消费 public C ABI。

## 文档维护

设计变化更新最接近的 design 文档和模块 README；实现偏离规范时记录偏差，不静默降低设计约束。
summary 是按请求维护的解释快照，不能因为代码改动自动更新。新增或修改跨模块能力前，先阅读本文、相关 design、模块 README 和规则文件。
