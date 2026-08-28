# ARCHITECTURE.md

本文是项目当前架构的唯一入口，只保留最高优先级约束和详细文档导航。当前顶层严格分为
`engine/`、`renderer/` 和 `app/`。

```text
app ──> renderer ──> engine
 │                      ▲
 └────> engine/platform ─┘
```

`renderer -> app` 和 `engine -> renderer` 都是禁止依赖。具体 crate 事实、状态 owner、时序和资源契约由
`docs/summaries/` 与模块 README 承载。

## 三层职责

- `engine/`：通用 Runtime、`RenderLoop`、`RenderThread`、RenderGraph、World、Vulkan RHI、窗口宿主及
  shader/CXX 基础设施。Engine 不知道任何具体 Renderer 或 Tauri。
- `renderer/`：具体 Renderer、Subsystem、Pass、Shader、产品 overlay 和传输无关的 typed ports。
  `TruvisRenderer`、Triangle、ShaderToy 和 Cornell Renderer 都属于此层。
- `app/`：Tauri Editor 与 standalone sample 的启动壳。Tauri `invoke/emit/AppHandle`、dialog、WebView、
  capabilities 和两秒 timeout 只存在此层。

## 推荐阅读顺序

1. [`layering-and-dependency-boundaries.md`](summaries/layering-and-dependency-boundaries.md)：顶层三层分层与 crate 依赖方向。
2. [`frame-lifecycle.md`](summaries/frame-lifecycle.md)：启动、统一帧执行器和 Runtime/Renderer/Subsystem phase。
3. [`runtime-renderer-subsystem-boundaries.md`](summaries/runtime-renderer-subsystem-boundaries.md)：状态 owner、Ctx 裁剪和静态子系统组合。
4. [`threading-and-resource-lifecycle.md`](summaries/threading-and-resource-lifecycle.md)：线程、GPU 同步和资源创建/重建/销毁契约。

| 主题 | 当前实现事实入口 |
| --- | --- |
| CPU Scene、asset identity、GPU scene 与 prepare | [`scene-data-lifecycle.md`](summaries/scene-data-lifecycle.md) |
| RenderGraph、pass 顺序、image 状态与提交 | [`render-graph-and-data-flow.md`](summaries/render-graph-and-data-flow.md) |
| Runtime/Renderer/Subsystem 配置 | [`render-configuration-system.md`](summaries/render-configuration-system.md) |
| Realtime RT、ReSTIR 与 SHARC | [`realtime-rt-raytracing-flow.md`](summaries/realtime-rt-raytracing-flow.md) |
| Tauri Web Editor、typed ports、背压与一致性 | [`editor-subsystem.md`](summaries/editor-subsystem.md) |

## 模块入口

- [`engine/README.md`](../engine/README.md)：Engine 目录与 crate 导航。
- [`renderer/README.md`](../renderer/README.md)：Renderer 层职责和组成。
- [`renderer/truvis/README.md`](../renderer/truvis/README.md)：`TruvisRenderer`、controller、ports 和 pass 编排。
- [`renderer/shader/README.md`](../renderer/shader/README.md)：Renderer shader package、ABI 和 binding owner。
- [`app/README.md`](../app/README.md)：Tauri 和 standalone 启动壳。
- [`app/editor/README.md`](../app/editor/README.md)：Web Editor 构建与 Tauri transport。
- [`truvis-render-thread/README.md`](../engine/e60-platform/truvis-render-thread/README.md)：窗口 backend 无关的渲染线程。
- [`truvis-winit-host/README.md`](../engine/e60-platform/truvis-winit-host/README.md)：standalone 和 embedded winit 宿主。
- [`docs/brain-storm/README.md`](brain-storm/README.md)：尚未进入主线的活跃设计方向。

## 全局约束

- `RenderRuntime` 拥有 `Gfx`、`World`、GPU resource/binding/timing owner、`RenderWorld`、present、command
  和同步资源；Renderer 与 Subsystem 只通过当前 phase 的窄 Ctx 使用能力。
- 具体 Renderer 拥有 camera/input、overlay、selection 和渲染子系统，并显式决定 RenderGraph pass 顺序。
  `SubsystemLifecycle` 只约束 init/resize/shutdown，controller 不实现该 trait。
- `truvis-renderer` 仅接收 `TruvisRendererPorts`。App 在 Tauri main thread 创建 ports，保留
  `TruvisFrontendPorts`，将 Renderer 侧 ports 移入 RenderThread factory。
- CPU scene 只由 `World`/`SceneStore` 权威持有；GPU scene 是 prepare 后的派生状态。
- Vulkan 对象只在 RenderThread 创建、使用和销毁。窗口 owner 持有 `RenderThread` handle，关闭时先回收
  Renderer/Runtime/Vulkan，再销毁 child HWND 和 Tauri parent。
- 当前 `shader-packages.toml` 声明的 owner 方向为 `renderer -> engine`；通用校验器从 package 依赖闭包、
  `shared_inputs.layer` 和 include root 推导可见边界，不内置 Engine/Renderer/sample 名称。
- Shader manifest、编译器和 binding codegen 位于 `engine/e00-utils/`；源码与 ABI owner 留在
  `engine/shader/`、`renderer/shader/`。`map.toml`/`truvis-path` 决定产物物理根，manifest 决定 package、
  binding 与输出前缀等逻辑路径，owner `build.rs` 决定 allowlist/re-export policy。

## 文档职责

- `docs/ARCHITECTURE.md`：当前架构入口与最高优先级约束。
- `docs/summaries/`：当前实现事实。
- 模块 README：模块职责、依赖、局部生命周期和常用入口。
- `docs/brain-storm/`：未实现但仍有工程价值的方向；完成后提炼事实并删除，不建立归档。
