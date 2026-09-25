# truvis-renderer

`truvis-renderer` 是主体产品 Renderer crate，负责组合 RendererClient 注入边界、UI/输入状态和
realtime/offline 渲染子系统。它依赖 engine 与公共 Renderer capability，但不依赖 Tauri 或 WebView。

## 主要职责

- `TruvisRenderer`：RenderThread 上的具体 `Renderer`，持有 camera/input、GUI、overlay、selection、注入的 RendererClient
  和 realtime/offline 渲染子系统，并显式决定 update 与 RenderGraph pass 顺序。
- `TruvisDlssState` 与 `ViewAccumState`：集中持有 Truvis 的 DLSS requested/effective 配置、NVIDIA capability、
  SR/RR resource lifecycle、Streamline options、temporal snapshot 和主视图累计状态；Streamline 初始化仍由
  Engine/Gfx 负责。DLSS feature 在 update/resize/shutdown 的 idle 边界释放，下一次 DLSS pass 在完成
  resource tags 后通过当前帧 evaluate 创建 feature。SR options 在配置边界设置，RR options 的相机矩阵
  由 `TruvisDlssState::frame_input` 逐帧更新，不因更新矩阵而释放 feature 或 reset history。
  尺寸改变时 update 只提交请求，释放、配置和 reset 统一在随后 resize 执行；仅配置改变时在 update
  执行。Offline 的 native 尺寸由 effective 配置统一派生，保留 requested 供返回 Realtime 恢复。
  SR/RR options 提交错误经 `Result` 传播到 Renderer hook，以阶段明确的 `expect` 终止渲染执行路径。
- `RendererClient`：App 提供的窄生命周期接口；Renderer 只在 init/update/after_prepare/shutdown 阶段调用它。
- `TruvisOverlayUi`：组合 `renderer-imgui` 的诊断控件与 `renderer-render-ui` 的设置 section，提供 Render、Sky、Post、Picking、Debug 五个固定 tab 和常驻 FPS HUD。
- `ViewportOverlaySubsystem`：统一持有灯光、transform gizmo 和坐标 gizmo 的唯一 pipeline、每 FIF vertex buffer 与最终顶点数组；`SelectionOutlineSubsystem` 独立持有网格描边资源。
- `LightOverlay`：CPU 图标投影、稳定排序、矩形命中及选中灯光辅助线段快照。
- `TransformGizmo`：CPU 轴投影、命中与平移计算，不认识 scene handle。`AxisDragState` 保存活动轴，Renderer 独立绑定拖动对象。
- `OverlayGeometry`：共用屏幕线段、实心箭头和三角形裁剪；坐标 gizmo 直接在此构造，不再单独持有 GPU subsystem。

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
- `SdrPostProcess` 由 Renderer 单独持有唯一实例，接收两种模式的 HDR 输出，共用曝光历史、AgX LUT 和显示 pass。
  显示设置由 `PathTracingCommonSettings` 持有，曝光参数在固定 update 中归一化；更改显示设置不重置 Offline spp。
- Renderer 不拥有 Editor endpoint 或 desktop command receiver；这些 receiver 属于 App Client。

## 运行与编排

compute 与 present graph 分别在局部作用域完成录制；compute 的资源借用结束后才进入 present 编排。
两种模式只选择各自的输入目标和命令缓冲，共用 SDR 后处理及 present/overlay 编排；GPU owner 保持独立。

`render` 返回主视图场景是否实际提交；无 TLAS 清屏返回 false。相机历史与 jitter 在提交后确认，
Loop 用同一结果确认 Runtime instance 历史，避免 prepare 或仅 present 推进运动历史。

`TruvisRenderer::render` 根据当前 `RenderMode` 选择 realtime 或 offline 渲染子系统，并显式组织主图 resolve、
selection outline、viewport overlay 与 ImGui 的顺序。具体 pass 位于 `renderer-render-passes`，渲染 owner 位于
`renderer-rendering`，ImGui 与设置控件分别位于 `renderer-imgui` 和 `renderer-render-ui`；`renderer-kit` 只提供基础契约和 CPU 状态。

## 灯光辅助显示

`LightOverlay` 在 update 从 CPU scene 构造无 jitter 投影，图标绘制和命中共用该快照。
图标为 24×24 viewport 物理像素，命中矩形为 28×28；远到近稳定绘制，逆序命中。
交互优先级为已有拖拽、UI/导航阻止新交互、transform gizmo、灯光图标、场景 GPU raycast。
轴命中即消费点击，即使不能建立拖动；hover 只显示最高优先级目标。
图标命中不提交 GPU 查询；新选择同帧刷新 gizmo 展示，但不重复消费按下事件。
输入保留按下时的物理像素位置，松开帧提交最终拖动位置；失焦、resize 或目标失效会取消拖动并清理按键。

图标与选中灯光线框使用无深度 alpha overlay，不进入光照、降噪或累计。
Point 的 0.25 m 球形标记、Spot 的 1 m 辅助射线长度不代表有限照射范围；
Area 使用真实矩形半轴和正面方向。线框只作显示，不参与拾取。
统一 pass 内按辅助线框、灯光图标、transform gizmo、坐标 gizmo 顺序追加顶点。
transform 轴保留 96 像素基准的世界轴长和投影缩短，箭杆宽 4 像素、箭头宽 14 像素、长 min(16, 轴屏幕长度 × 0.4)。
短于 8 像素的轴不画也不命中；命中半径 10 像素，等距离选择后绘制的轴。裁剪掉原终点时只画箭杆。
坐标 gizmo 保留 112 像素区域和 24 像素边距，在 CPU 裁剪到该区域。

update 在交互前生成命中数据，成功 mutation 后刷新展示，并固定本帧 `RenderView`。
`commit_selection` 集中比较、通知和废弃旧 gizmo；普通选择不清空输入。after_prepare 改选网格时下一次 update 才生成新 gizmo。
render 依据最终 selection 从空数组构建颜色和顶点，不读取 World，也不再次消费输入。
prepare、overlay 与 Offline 累计签名共用同一视图；after_prepare 相机查询结果下一帧生效。

每个 FIF 独立 vertex buffer，在当前 label 完成等待后写入或按需扩容，shutdown 先于 Runtime 回收。
仅 DLSS 内部尺寸变化不取消交互；真实输出 extent 变化取消拖动和旧展示。普通 resize 保留 buffer，present format 变化才重建 pipeline。

## 边界约束

- 不把 Tauri、WebView、Renderer overlay 或具体渲染子系统策略下沉到 engine crate。
- 不让 WebView、Editor IPC owner 或 Tauri main thread 直接访问 `GameWorld` 或 Vulkan 对象。
- 不把本机文件路径放入通用 Editor DTO。
- 不绕过唯一 `RenderLoop` 帧骨架，也不让 Renderer/子系统长期持有完整 runtime 或 typed `Gfx` Ctx。
- 主体 Renderer 的 pass 顺序、selection/overlay 策略和 realtime/offline 模式选择不进入 `SubsystemLifecycle`。

跨线程 Editor、协议与一致性边界见 [`docs/design/editor-boundary-and-consistency.md`](../../docs/design/editor-boundary-and-consistency.md)。
Runtime/Renderer/Subsystem 的通用契约见
[`docs/design/runtime-phase-contract.md`](../../docs/design/runtime-phase-contract.md)。

## 场景环境编辑

Sky tab 的 enabled/brightness 从 World 读取为本次 UI 临时值，修改后经 GameWorld 原子提交；
PathTracingCommonSettings 只保留采样策略，不缓存环境倍率。Web Environment Inspector 与 ImGui 共用同一权威。
灯光 Inspector 与 Gizmo 共用字段级 World mutation，图标、线框和位置继续读取既有三张灯光表。
