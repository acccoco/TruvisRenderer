# truvis

`truvis` 是主体桌面应用 crate，负责组合 Tauri WebView、嵌入式原生 viewport、Editor、App-owned UI/输入状态
以及 realtime/offline 渲染管线。它依赖 engine 与 app 公共组件，但不向下层注入主体 App 业务语义。

## 主要职责

- `desktop`：Tauri/Tao main-thread owner，组装 WebView、EditorServer、desktop command 与
  `truvis-winit-host::EmbeddedWinitHost`，并保证 parent window 最后销毁。
- `TruvisRenderApp`：RenderThread 上的具体 `RenderApp`，持有 camera/input、GUI、overlay、selection、Editor controller
  和 realtime/offline pipeline，并显式决定 update 与 RenderGraph pass 顺序。
- `EditorController`：把 Editor 协议 DTO 适配到权威 `World` 查询与 edit API。
- `DesktopCommandController`：消费 Tauri 本地特权命令，只把 Rust `PathBuf` 交给 `World`，不扩展 WebSocket 协议。
- `TruvisOverlayUi`：组合 app-kit 提供的 UI section，决定主体 App 的窗口布局和绘制顺序。
- `SelectionOutlineRenderer` / `CoordinateGizmoRenderer`：持有主体 App 专用效果的资源与 pass 编排状态。

## 状态所有权

- CPU scene 权威状态属于 runtime-owned `World`；App 只在合法 update 阶段通过 `World` facade 修改它。
- 当前 selection 属于 `TruvisRenderApp`，保存 `InstanceHandle + submesh_index`，不保存 GPU instance slot。
- camera、input、overlay 和 debug image 选择属于 App；runtime 只消费 `RenderView` 或稳定选择语义。
- realtime `RtPipeline` 与 `OfflinePipeline` 都由 App 持有。两者拥有各自 target、累计和 temporal 状态，
  不把窗口尺寸资源下沉到 `RenderRuntime`。
- EditorServer 生命周期属于 desktop owner；RenderThread 中的 App 只持有协议 endpoint 与 desktop command receiver。

## 运行与编排

主体入口通过 `just truvis` 构建 Web、shader 和 Debug CXX 产物后启动；`just truvis-direct` 只构建 Web，
适合确认 shader/CXX 产物已经最新时使用。两个入口都可追加 `imgui` 或 `no-validation`。

`TruvisRenderApp::render` 根据当前 `RenderMode` 选择 realtime 或 offline pipeline，并显式组织主图 resolve、
selection outline、coordinate gizmo 与 GUI 的顺序。具体 pass 实现在 `app-render-passes`，公共 pipeline glue 与
GUI/input/overlay 组件位于 `app-kit`；本 crate 只负责主体 App 的业务组合。

## 边界约束

- 不把 Tauri、Editor DTO、App overlay 或具体 pipeline 策略下沉到 engine crate。
- 不让 WebView、EditorServer 或 Tauri main thread 直接访问 `World` 或 Vulkan 对象。
- 不把本机文件路径放入 Editor WebSocket DTO。
- 不绕过唯一 `RenderAppRunner` 帧骨架，也不让 App/子系统长期持有完整 runtime 或 typed `Gfx` Ctx。
- 主体 App 的 pass 顺序、selection/overlay 策略和 realtime/offline 模式选择不进入 `SubsystemLifecycle`。

跨线程 Editor、协议与一致性边界见 [`docs/summaries/editor-subsystem.md`](../../docs/summaries/editor-subsystem.md)。
Runtime/App/Subsystem 的通用契约见
[`docs/summaries/runtime-app-subsystem-boundaries.md`](../../docs/summaries/runtime-app-subsystem-boundaries.md)。
