# truvis-renderer

`truvis-renderer` 是主体产品 Renderer crate，负责组合 Editor 渲染侧 endpoint、UI/输入状态和
realtime/offline 渲染子系统。它依赖 engine 与公共 Renderer capability，但不依赖 Tauri 或 WebView。

## 主要职责

- `TruvisRenderer`：RenderThread 上的具体 `Renderer`，持有 camera/input、GUI、overlay、selection、Editor controller
  和 realtime/offline 渲染子系统，并显式决定 update 与 RenderGraph pass 顺序。
- `StartupScene`：选择 manifest 或兼容的 Bistro 初始场景，把场景文件、相机 preset 和面光源请求交给 `World`；
  `SceneManifest` 位于 `truvis-asset`，不包含 Renderer/App 状态。`TRUVIS_SCENE_MANIFEST` 选择 manifest；
  旧 Bistro 环境变量仍只在兼容适配文件中解析。只跟踪 CPU import 状态，GPU ready 属于 Runtime。
- `EditorController`：把 Editor 协议 DTO 适配到权威 `World` 查询与 edit API。
- `DesktopCommandController`：消费 Tauri 本地特权命令，只把 Rust `PathBuf` 交给 `World`，不扩展通用 Editor DTO。
- `TruvisOverlayUi`：组合 `renderer-imgui` 的诊断控件与 `renderer-render-ui` 的设置 section，决定主体 Renderer 的窗口布局和绘制顺序。
- `SelectionOutlineSubsystem` / `CoordinateGizmoSubsystem`：持有主体 Renderer 专用效果的资源与 pass 编排状态。

## 状态所有权

- CPU scene 权威状态属于 runtime-owned `World`；Renderer 只在合法 update 阶段通过 `World` facade 修改它。
- 当前 selection 属于 `TruvisRenderer`，保存 `InstanceHandle + submesh_index`，不保存 GPU instance slot。
- camera、input、overlay 和 debug image 选择属于 Renderer；runtime 只消费 `RenderView` 或稳定选择语义。
- `RealtimeRenderSubsystem` 与 `OfflineRenderSubsystem` 都由 Renderer 持有。两者拥有各自 target、累计和 temporal 状态，
  不把窗口尺寸资源下沉到 `RenderRuntime`。
- Renderer 只持有 `TruvisRendererPorts`，其中包含传输无关的 `RendererEndpoint` 与 desktop command receiver。

## 运行与编排

先执行根目录 `just prepare-bistro <Bistro_v5_2目录>`，然后使用 `just truvis`、
`just truvis bistro-exterior` 或 `just truvis bistro-interior-wine`。
初始相机来自 FBX 相机，移动速度为每秒 4 个场景单位；默认 Offline 累计并关闭 debug image 覆盖。
`TRUVIS_RENDER_MODE` 仍可覆盖产品初始模式，overlay 可随时切换 Realtime/Offline。
资源转换与材质限制见根 [README](../../README.md)。

`TruvisRenderer::render` 根据当前 `RenderMode` 选择 realtime 或 offline 渲染子系统，并显式组织主图 resolve、
selection outline、coordinate gizmo 与 ImGui 的顺序。具体 pass 位于 `renderer-render-passes`，渲染 owner 位于
`renderer-rendering`，ImGui 与设置控件分别位于 `renderer-imgui` 和 `renderer-render-ui`；`renderer-kit` 只提供基础契约和 CPU 状态。

## 边界约束

- 不把 Tauri、WebView、Renderer overlay 或具体渲染子系统策略下沉到 engine crate。
- 不让 WebView、Editor IPC owner 或 Tauri main thread 直接访问 `World` 或 Vulkan 对象。
- 不把本机文件路径放入通用 Editor DTO。
- 不绕过唯一 `RenderLoop` 帧骨架，也不让 Renderer/子系统长期持有完整 runtime 或 typed `Gfx` Ctx。
- 主体 Renderer 的 pass 顺序、selection/overlay 策略和 realtime/offline 模式选择不进入 `SubsystemLifecycle`。

跨线程 Editor、协议与一致性边界见 [`docs/summaries/editor-subsystem.md`](../../docs/summaries/editor-subsystem.md)。
Runtime/Renderer/Subsystem 的通用契约见
[`docs/summaries/runtime-renderer-subsystem-boundaries.md`](../../docs/summaries/runtime-renderer-subsystem-boundaries.md)。
