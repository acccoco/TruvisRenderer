# truvis-renderer

`truvis-renderer` 是主体产品 Renderer crate，负责组合 RendererClient 注入边界、UI/输入状态和
realtime/offline 渲染子系统。它依赖 engine 与公共 Renderer capability，但不依赖 Tauri 或 WebView。

## 主要职责

- `TruvisRenderer`：RenderThread 上的具体 `Renderer`，持有 camera/input、GUI、overlay、selection、注入的 RendererClient
  和 realtime/offline 渲染子系统，并显式决定 update 与 RenderGraph pass 顺序。
- `RendererClient`：App 提供的窄生命周期接口；Renderer 只在 init/update/after_prepare/shutdown 阶段调用它。
- `TruvisOverlayUi`：组合 `renderer-imgui` 的诊断控件与 `renderer-render-ui` 的设置 section，提供 Render、Sky、Post、Picking、Debug 五个固定 tab 和常驻 FPS HUD。
- `LightOverlaySubsystem` / `SelectionOutlineSubsystem` / `TransformGizmoSubsystem` / `CoordinateGizmoSubsystem`：持有主体 Renderer 专用效果的资源与 pass 编排状态。
  `TransformGizmoSubsystem` 只负责三轴 CPU 命中、拖拽约束和 overlay 绘制数据；它不认识 scene handle 或 raycast service。
  hover 不提交 GPU raycast，左键命中轴后由 Renderer 拦截场景选择请求，拖拽期间保持同一 instance 或带类型灯光身份绑定，失焦、resize 或目标失效时结束交互。

Truvis Tauri App 与 Cornell standalone App 共用完整 Renderer，不按宿主裁剪渲染功能。
两个 App 均通过 `truvis-scenes` 选择 `--scene manual|sponza|cornell`，默认 `manual`。
`TruvisAppClient` 组合场景、Editor 请求和本地桌面命令；`CornellAppClient` 只组合场景。
两者都在 RenderThread 上通过 `RendererClient` 被调用。
Renderer 本身只消费 CPU scene 结果并负责 GPU/RenderGraph 编排。

## 状态所有权

- Overlay 主窗口顶部常驻 Render Mode 与 Offline Samples，各 tab 的独立滚动状态由 ImGui context 保存；`TruvisOverlayOptions` 只控制主窗口与 FPS HUD 的显示，不提供多布局配置或磁盘持久化。
- Offline settings 与 Debug Image 选择在 Renderer 固定 update 路径归一化；折叠窗口或切换 tab 不停止 picking、不清除 selection，也不关闭已经启用的 debug image 输出。
- CPU scene 权威状态属于 runtime-owned `GameWorld`；Renderer 只在合法 update 阶段通过 `GameWorld` facade 修改它。
- 当前 selection 属于 `TruvisRenderer`，保存 `SceneSelection::Submesh` 或 `SceneSelection::Light(LightTarget)`，不保存 GPU slot 或材质缓存。
- Transform gizmo 是 Renderer-owned interaction。`TruvisRenderer` 在 update 阶段绑定当前 selection，拖拽期间直接调用
  `GameWorld::update_instance_transform` 或 `update_light_position`；下一次 prepare 同步到 `RenderWorld`。
- camera、input、overlay 和 debug image 选择属于 Renderer；runtime 只消费 `RenderView` 或稳定选择语义。
- `RealtimeRenderSubsystem` 与 `OfflineRenderSubsystem` 都由 Renderer 持有。两者拥有各自 target、累计和 temporal 状态，
  不把窗口尺寸资源下沉到 `RenderRuntime`。
- Renderer 不拥有 Editor endpoint 或 desktop command receiver；这些 receiver 属于 App Client。

## 运行与编排

`TruvisRenderer::render` 根据当前 `RenderMode` 选择 realtime 或 offline 渲染子系统，并显式组织主图 resolve、
selection outline、light overlay、transform gizmo、coordinate gizmo 与 ImGui 的顺序。具体 pass 位于 `renderer-render-passes`，渲染 owner 位于
`renderer-rendering`，ImGui 与设置控件分别位于 `renderer-imgui` 和 `renderer-render-ui`；`renderer-kit` 只提供基础契约和 CPU 状态。

## 灯光辅助显示

`LightOverlaySubsystem` 在 update 从 CPU scene 构造无 jitter 投影，图标绘制和命中共用该快照。
图标为 24×24 viewport 物理像素，命中矩形为 28×28；远到近稳定绘制，逆序命中。
交互优先级为已有拖拽/UI、transform gizmo、灯光图标、场景 GPU raycast；相机导航期间不开始选择。
图标命中不提交 GPU 查询；新选择同帧刷新 gizmo 展示，但不重复消费按下事件。
输入保留按下时的物理像素位置，松开帧提交最终拖动位置；失焦、resize 或目标失效会取消拖动并清理按键。

图标与选中灯光线框使用无深度 alpha overlay，不进入光照、降噪或累计。
Point 的 0.25 m 球形标记、Spot 的 1 m 辅助射线长度不代表有限照射范围；
Area 使用真实矩形半轴和正面方向。线框只作显示，不参与拾取。
每个 FIF 独立 vertex buffer，在当前 label 完成等待后写入或扩容，shutdown 先于 Runtime 回收。

## 边界约束

- 不把 Tauri、WebView、Renderer overlay 或具体渲染子系统策略下沉到 engine crate。
- 不让 WebView、Editor IPC owner 或 Tauri main thread 直接访问 `GameWorld` 或 Vulkan 对象。
- 不把本机文件路径放入通用 Editor DTO。
- 不绕过唯一 `RenderLoop` 帧骨架，也不让 Renderer/子系统长期持有完整 runtime 或 typed `Gfx` Ctx。
- 主体 Renderer 的 pass 顺序、selection/overlay 策略和 realtime/offline 模式选择不进入 `SubsystemLifecycle`。

跨线程 Editor、协议与一致性边界见 [`docs/design/editor-boundary-and-consistency.md`](../../docs/design/editor-boundary-and-consistency.md)。
Runtime/Renderer/Subsystem 的通用契约见
[`docs/design/runtime-phase-contract.md`](../../docs/design/runtime-phase-contract.md)。
