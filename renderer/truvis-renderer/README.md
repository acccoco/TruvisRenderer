# truvis-renderer

`truvis-renderer` 是主体产品 Renderer crate，负责组合 RendererClient 注入边界、UI/输入状态和
realtime/offline 渲染子系统。它依赖 engine 与公共 Renderer capability，但不依赖 Tauri 或 WebView。

## 主要职责

- `TruvisRenderer`：RenderThread 上的具体 `Renderer`，持有 camera/input、GUI、overlay、selection、注入的 RendererClient
  和 realtime/offline 渲染子系统，并显式决定 update 与 RenderGraph pass 顺序。
- `RendererClient`：App 提供的窄生命周期接口；Renderer 只在 init/update/after_prepare/shutdown 阶段调用它。
- `TruvisOverlayUi`：组合 `renderer-imgui` 的诊断控件与 `renderer-render-ui` 的设置 section，决定主体 Renderer 的窗口布局和绘制顺序。
- `SelectionOutlineSubsystem` / `CoordinateGizmoSubsystem`：持有主体 Renderer 专用效果的资源与 pass 编排状态。

Truvis Tauri App 与 Cornell standalone App 共用完整 Renderer，不按宿主裁剪渲染功能。
两个 App 均通过 `truvis-scenes` 选择 `--scene manual|sponza|cornell`，默认 `manual`。
`TruvisAppClient` 组合场景、Editor 请求和本地桌面命令；`CornellAppClient` 只组合场景。
两者都在 RenderThread 上通过 `RendererClient` 被调用。
Renderer 本身只消费 CPU scene 结果并负责 GPU/RenderGraph 编排。

## 状态所有权

- CPU scene 权威状态属于 runtime-owned `GameWorld`；Renderer 只在合法 update 阶段通过 `GameWorld` facade 修改它。
- 当前 selection 属于 `TruvisRenderer`，保存 `MeshInstanceHandle + submesh_index`，不保存 GPU instance slot。
- camera、input、overlay 和 debug image 选择属于 Renderer；runtime 只消费 `RenderView` 或稳定选择语义。
- `RealtimeRenderSubsystem` 与 `OfflineRenderSubsystem` 都由 Renderer 持有。两者拥有各自 target、累计和 temporal 状态，
  不把窗口尺寸资源下沉到 `RenderRuntime`。
- Renderer 不拥有 Editor endpoint 或 desktop command receiver；这些 receiver 属于 App Client。

## 运行与编排

`TruvisRenderer::render` 根据当前 `RenderMode` 选择 realtime 或 offline 渲染子系统，并显式组织主图 resolve、
selection outline、coordinate gizmo 与 ImGui 的顺序。具体 pass 位于 `renderer-render-passes`，渲染 owner 位于
`renderer-rendering`，ImGui 与设置控件分别位于 `renderer-imgui` 和 `renderer-render-ui`；`renderer-kit` 只提供基础契约和 CPU 状态。

## 边界约束

- 不把 Tauri、WebView、Renderer overlay 或具体渲染子系统策略下沉到 engine crate。
- 不让 WebView、Editor IPC owner 或 Tauri main thread 直接访问 `GameWorld` 或 Vulkan 对象。
- 不把本机文件路径放入通用 Editor DTO。
- 不绕过唯一 `RenderLoop` 帧骨架，也不让 Renderer/子系统长期持有完整 runtime 或 typed `Gfx` Ctx。
- 主体 Renderer 的 pass 顺序、selection/overlay 策略和 realtime/offline 模式选择不进入 `SubsystemLifecycle`。

跨线程 Editor、协议与一致性边界见 [`docs/design/editor-boundary-and-consistency.md`](../../docs/design/editor-boundary-and-consistency.md)。
Runtime/Renderer/Subsystem 的通用契约见
[`docs/design/runtime-phase-contract.md`](../../docs/design/runtime-phase-contract.md)。
