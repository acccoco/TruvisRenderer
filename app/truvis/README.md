# truvis

`truvis` 是主体桌面应用 crate，负责组合 Tauri WebView、嵌入式原生 viewport、Editor，以及由
`TruvisRenderer` 持有的 UI/输入状态和 realtime/offline 渲染子系统。它依赖 engine 与 app 公共组件，
但不向下层注入主体渲染业务语义。

## 主要职责

- `desktop`：Tauri/Tao main-thread owner，组装 WebView、EditorServer、desktop command 与
  `truvis-winit-host::EmbeddedWinitHost`，并保证 parent window 最后销毁。
- `TruvisRenderer`：RenderThread 上的具体 `Renderer`，持有 camera/input、GUI、overlay、selection、Editor controller
  和 realtime/offline 渲染子系统，并显式决定 update 与 RenderGraph pass 顺序。
- `EditorController`：把 Editor 协议 DTO 适配到权威 `World` 查询与 edit API。
- `DesktopCommandController`：消费 Tauri 本地特权命令，只把 Rust `PathBuf` 交给 `World`，不扩展 WebSocket 协议。
- `TruvisOverlayUi`：组合 `app-imgui` 的诊断控件与 `app-render-ui` 的设置 section，决定主体 Renderer 的窗口布局和绘制顺序。
- `SelectionOutlineSubsystem` / `CoordinateGizmoSubsystem`：持有主体 Renderer 专用效果的资源与 pass 编排状态。

## 状态所有权

- CPU scene 权威状态属于 runtime-owned `World`；Renderer 只在合法 update 阶段通过 `World` facade 修改它。
- 当前 selection 属于 `TruvisRenderer`，保存 `InstanceHandle + submesh_index`，不保存 GPU instance slot。
- camera、input、overlay 和 debug image 选择属于 Renderer；runtime 只消费 `RenderView` 或稳定选择语义。
- `RealtimeRenderSubsystem` 与 `OfflineRenderSubsystem` 都由 Renderer 持有。两者拥有各自 target、累计和 temporal 状态，
  不把窗口尺寸资源下沉到 `RenderRuntime`。
- EditorServer 生命周期属于 desktop owner；RenderThread 中的 Renderer 只持有 `AppEndpoint` 与 desktop command receiver。

## 运行与编排

主体入口通过 `just truvis` 构建 Web、shader 和 Debug CXX 产物后启动；`just truvis-direct` 只构建 Web，
适合确认 shader/CXX 产物已经最新时使用。两个入口都可追加 `imgui` 或 `no-validation`。

`TruvisRenderer::render` 根据当前 `RenderMode` 选择 realtime 或 offline 渲染子系统，并显式组织主图 resolve、
selection outline、coordinate gizmo 与 ImGui 的顺序。具体 pass 位于 `app-render-passes`，渲染 owner 位于
`app-rendering`，ImGui 与设置控件分别位于 `app-imgui` 和 `app-render-ui`；`app-kit` 只提供基础契约和 CPU 状态。

## 边界约束

- 不把 Tauri、Editor DTO、Renderer overlay 或具体渲染子系统策略下沉到 engine crate。
- 不让 WebView、EditorServer 或 Tauri main thread 直接访问 `World` 或 Vulkan 对象。
- 不把本机文件路径放入 Editor WebSocket DTO。
- 不绕过唯一 `RenderLoop` 帧骨架，也不让 Renderer/子系统长期持有完整 runtime 或 typed `Gfx` Ctx。
- 主体 Renderer 的 pass 顺序、selection/overlay 策略和 realtime/offline 模式选择不进入 `SubsystemLifecycle`。

跨线程 Editor、协议与一致性边界见 [`docs/summaries/editor-subsystem.md`](../../docs/summaries/editor-subsystem.md)。
Runtime/Renderer/Subsystem 的通用契约见
[`docs/summaries/runtime-renderer-subsystem-boundaries.md`](../../docs/summaries/runtime-renderer-subsystem-boundaries.md)。
