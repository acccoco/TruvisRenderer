# Editor 子系统边界与一致性

> 状态：当前实现事实总结。本文记录 Tauri WebView、Editor IPC、主体 Renderer 与 CPU `World`
> 之间的状态所有权、协议、线程、背压和生命周期边界。

## 系统定位

Truvis Editor 由 Tauri/Tao 顶层窗口、React WebView、Windows child HWND 中的 Vulkan viewport，
以及 RenderThread 内的主体 Renderer 共同组成。Editor 属于 app 域，不进入 `engine/`；
`truvis-winit-host` 提供平台窗口，`truvis-render-thread` 提供 backend-independent 渲染线程宿主。

核心设计是让 UI 和 GPU scene 都不能成为第二份 CPU scene 权威状态：

- `World` / `SceneStore` 是 scene、material、sky 与 light 的唯一 CPU 权威 owner。
- 当前 selection 属于 `TruvisRenderer`，使用 CPU `InstanceHandle + submesh_index` 语义。
- GPU scene 是 `RenderRuntime::prepare` 根据 CPU scene 生成的派生状态。
- WebView 只保存可丢弃的展示投影；刷新后通过查询重新构建。
- Editor IPC 只短暂承载 owned DTO 和每请求 reply，不缓存场景快照。

## 依赖与职责边界

```text
React WebView
  -> Tauri invoke
truvis::editor_ipc
  -> truvis-editor-bridge
       <- truvis::editor_controller
            -> truvis-world

Editor notification
  -> bounded channel
  -> Tauri event
  -> React WebView

Tauri desktop command
  -> truvis::desktop_command
       -> truvis-world

DOM viewport rect
  -> truvis-winit-host::EmbeddedWinitHost
       -> child HWND
       -> truvis-render-thread::RenderThread
```

主要约束：

- `truvis-editor-bridge` 定义协议 DTO、每请求 oneshot reply 和方向受限的有界 endpoint；它不依赖
  `truvis-world`、render runtime、Tauri 或 GPU 类型。
- `truvis::editor_ipc` 只负责 invoke、通知转发、背压和 timeout，不解释领域请求。
- `app/truvis/capabilities/main-editor.json` 只授予 `main` WebView 注册和移除 Tauri event listener 的权限；
  页面不能通过 Tauri event API 向 native 侧发送业务消息。
- `truvis::editor_controller` 是协议 DTO 到 `World` handle、查询和 edit API 的唯一适配点。
- `truvis::desktop_command` 只处理 Tauri 本地特权命令。本机 `PathBuf` 不进入通用 Editor DTO。
- Web 只提交 child viewport 的物理像素矩形，不转发 viewport 鼠标或键盘输入；原生 child HWND 直接接收输入。

## 协议与身份

Rust `truvis-editor-bridge::protocol` 是 DTO 权威来源，TypeScript 通过 `ts-rs` 生成。Tauri invoke
原生关联一次请求与返回值，因此协议不维护 client identity、request identity 或版本协商。

协议区分四类消息：

- Query：读取当前权威状态，不建立持久 snapshot。
- Command：请求修改权威状态，并返回应用结果。
- Response：本次 invoke 对应的查询或命令结果。
- Notification：best-effort 状态变化提示，允许丢失。

Editor ID 是带类型的 opaque string，由对应 SlotMap key 可逆编码。WebView 不能解析 index/generation，
也不能把 `InstanceId`、`MeshId`、`MaterialId` 或 `TextureId` 互换。ID 不表示 GPU slot，
也不保证跨进程 session 稳定；删除或 generation 失效后由当前 `SceneStore` 返回 stale/not-found 错误。

`scene_version` 来自 `SceneStore` 的单调 `u64`，在 DTO 中使用十进制字符串避免 JavaScript number 精度损失。
只有成功且非 no-op 的 CPU scene 语义变化才推进版本；frame id、GPU manager revision 或 notification 数量都不能替代它。

## 查询、编辑与恢复

WebView 初始化后主动查询 scene version、selection、对象分页和所需详情。对象列表响应携带
`scene_version`；分页请求可以携带期望版本，版本冲突时页面丢弃不完整结果并从第一页重新查询。

材质编辑由 WebView invoke `UpdateMaterial` command。`EditorController` 在 Renderer update 阶段把 opaque ID
还原为强类型 handle，再调用 `World` edit API。校验失败不修改 `World`，成功后响应携带新的 scene version
和权威材质投影。

selection 在原生 viewport 中通过 runtime-owned 同步 raycast 得到。Renderer 保存 CPU 选择语义，并在变化后发送
best-effort notification；WebView 同时保留主动查询和一秒 version 轮询，因此 notification 丢失不会永久破坏投影。

HDRI 文件选择不是通用 Editor DTO。Tauri 打开本地文件对话框，通过进程内私有有界队列把 `PathBuf`
交给 RenderThread；WebView 只得到文件名和 accepted/cancelled/error。accepted 只表示 `World` 已接受
CPU scene 请求，不表示 decode、GPU upload 或 Alias distribution 已完成。

## 并发与背压

Desktop 到 Renderer 的 request inbox 容量为 `256`，Renderer 到 Desktop 的 notification outbox 容量为 `64`。
每个 request 携带独立 oneshot reply，不存在共享 response queue。Tauri command 使用 `try_send`，队列满返回 `busy`，
等待超过两秒返回 `timeout`；Renderer 关闭时 reply sender drop，使等待者立即结束。

RenderThread 只使用非阻塞 endpoint 操作，并按每帧最多 `32` 条、最多 `500 μs` 处理请求。notification 队列满时
允许丢弃。任何一侧都不能为了等待另一侧而阻塞 RenderThread、Tauri main thread 或持有 desktop resource lock。

通知 receiver 由 Tauri async runtime 中的单个 dispatcher task 消费，并定向发送给 `main` WebView；该 task
不是新的 OS 线程 owner，也不访问 `World` 或 Vulkan。

## 生命周期与当前限制

主体桌面运行时包含三个主要线程 owner：

- Tauri main thread：顶层窗口、WebView、文件对话框和 desktop state。
- RenderWindowThread：winit event loop 与 Windows child HWND。
- RenderThread：`RenderLoop`、`TruvisRenderer`、`World` 与所有 Vulkan 对象。

关闭时先拒绝新的 Editor/desktop 请求，再完成 Renderer、具体子系统、RenderRuntime 和 Vulkan 资源释放；
RenderThread 退出后销毁 child HWND，随后停止 notification dispatcher，最后销毁 Tauri parent window。

当前限制：

- notification 不重放；恢复依赖查询、version 轮询或页面刷新。
- 多次查询不保证来自同一个原子 scene snapshot。
- 窗口最小化导致 Renderer frame 停止时，Editor 请求可能 timeout；当前没有独立 CPU request pump。
- embedded viewport 当前是 Windows child HWND 实现，native viewport 上方不能叠加 WebView DOM。
- `?mock=1` 只用于 Vite 视觉验收，不访问真实 Renderer；真实 Editor 只允许在 Tauri WebView 中运行。

## 代码与操作入口

- 协议与 endpoint：`app/editor/bridge/src/`
- Tauri IPC owner：`app/truvis/src/editor_ipc.rs`
- WebView event capability：`app/truvis/capabilities/main-editor.json`
- RenderThread 协议适配：`app/truvis/src/editor_controller.rs`
- 页面 transport：`app/editor/web/src/transport/`
- 开发、构建和运行参数：`app/editor/README.md`
