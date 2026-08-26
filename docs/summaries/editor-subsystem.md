# Editor 子系统边界与一致性

> 状态：当前实现事实总结。本文记录 Tauri WebView、EditorServer、EditorBridge、主体 Renderer 与 CPU `World`
> 之间的状态所有权、协议、线程、背压和生命周期边界。

## 系统定位

Truvis Editor 由 Tauri/Tao 顶层窗口、React WebView、Windows child HWND 中的 Vulkan viewport、
EditorServer 和 RenderThread 内的主体 Renderer 共同组成。Editor 属于 app 域，不进入 `engine/`；
`engine/e60-platform/truvis-winit-host` 提供平台窗口，`engine/e60-platform/truvis-render-thread` 提供 backend-independent
渲染线程宿主；两者都不依赖 Web、Tauri 或 Editor 协议。

核心设计是让 UI、网络和 GPU scene 都不能成为第二份 CPU scene 权威状态：

- `World` / `SceneStore` 是 scene、material、sky 与 light 的唯一 CPU 权威 owner。
- 当前 selection 属于 `TruvisRenderer`，使用 CPU `InstanceHandle + submesh_index` 语义。
- GPU scene 是 `RenderRuntime::prepare` 根据 CPU scene 生成的派生状态。
- Web 页面只保存可丢弃的展示投影；刷新或建立新 client session 后通过查询重新构建。
- EditorServer 和 EditorBridge 只短暂承载 owned DTO，不缓存场景快照或维护权威状态。

## 依赖与职责边界

```text
React Web
  <-> HTTP / WebSocket
EditorServer
  -> truvis-editor-bridge
       <- truvis::editor_controller
            -> truvis-world

Tauri desktop command
  -> truvis::desktop_command
       -> truvis-world

DOM viewport rect
  -> truvis-winit-host::EmbeddedWinitHost
       -> child HWND
       -> truvis-render-thread::RenderThread
```

主要约束：

- `truvis-editor-bridge` 定义协议 DTO、transport envelope 和方向受限的有界 endpoint；它不依赖
  `truvis-world`、render runtime 或 GPU 类型。
- `truvis-editor-server` 只负责 loopback HTTP/WebSocket、JSON、client 路由和静态文件；它只依赖 Bridge。
- `truvis::editor_controller` 是协议 DTO 到 `World` handle、查询和 edit API 的唯一适配点。
- `truvis::desktop_command` 只处理 Tauri 本地特权命令。本机 `PathBuf` 不进入 WebSocket DTO，也不经过 EditorServer。
- Web 只提交 child viewport 的物理像素矩形，不转发 viewport 鼠标或键盘输入；原生 child HWND 直接接收这些输入。

## 协议与身份

Rust `truvis-editor-bridge::protocol` 是协议权威来源，TypeScript 通过 `ts-rs` 生成。Server 只增加
`ClientId` 等 transport 路由信息，不重新定义 scene DTO。

协议区分四类消息：

- Query：读取当前权威状态，不建立持久 snapshot。
- Command：请求修改权威状态，并返回应用结果。
- Response：与 `request_id` 对应的查询或命令结果。
- Notification：best-effort 状态变化提示，允许丢失。

Editor ID 是带类型的 opaque string，由对应 SlotMap key 可逆编码。Web 不能解析 index/generation，
也不能把 `InstanceId`、`MeshId`、`MaterialId` 或 `TextureId` 互换。ID 不表示 GPU slot，
也不保证跨进程 session 稳定；删除或 generation 失效后由当前 `SceneStore` 返回 stale/not-found 错误。

`scene_version` 来自 `SceneStore` 的单调 `u64`，在线协议中使用十进制字符串避免 JavaScript number 精度损失。
只有成功且非 no-op 的 CPU scene 语义变化才推进版本；frame id、GPU manager revision 或 notification 数量都不能替代它。

## 查询、编辑与恢复流程

Web 初次连接后主动查询 capabilities、scene version、selection、对象分页和所需详情。对象列表响应携带
`scene_version`；分页请求可以携带期望版本，版本冲突时页面丢弃不完整结果并从第一页重新查询。

材质编辑由 Web 发送 `UpdateMaterial` command。`EditorController` 在 Renderer update 阶段把 opaque ID 还原为
强类型 handle，再调用 `World` edit API。校验失败不修改 `World`，成功后响应携带新的 scene version 和权威材质投影。

selection 在原生 viewport 中通过 runtime-owned 同步 raycast 得到。Renderer 保存 CPU 选择语义，并在变化后发送
best-effort notification；Web 同时保留主动查询能力，因此 notification 丢失不会永久破坏投影。

HDRI 文件选择不是 Editor 网络协议。Tauri main thread 打开本地文件对话框，通过进程内私有有界队列把
`PathBuf` 交给 RenderThread；Web 只得到文件名和 accepted/cancelled/error。accepted 只表示 `World` 已接受
CPU scene 请求，不表示 decode、GPU upload 或 Alias distribution 已完成。

当前一致性模型是最终一致性：

- Server 不缓存 scene 查询结果，RenderThread 不建立持久 snapshot session。
- 多次查询可能观察到不同版本，页面通过 `scene_version`、主动查询和周期轮询恢复。
- 页面刷新视为新 client session，不重放旧 notification。
- response 与迟到详情由 request id 或页面内 request sequence 关联；页面必须丢弃不再匹配当前选择的结果。

## 并发与背压

EditorServer 在独立 OS 线程中的 Tokio current-thread runtime 上运行；网络 IO、JSON 序列化和静态文件服务
都不进入 RenderThread。Server 到 `AppEndpoint` 的 request inbox、`AppEndpoint` 到 Server 的 response/notification outbox 和
Tauri desktop command queue 都是有界通道。

RenderThread 只使用非阻塞 endpoint 操作，并按每帧数量与时间预算处理请求。队列满时 Server 返回 `busy`；
notification 队列满时允许丢弃；response 无法发送时由 Web timeout 收敛为失败。任何一侧都不能为了等待另一侧
而阻塞 RenderThread、Tauri main thread 或持有 desktop resource lock。

## 线程与生命周期

主体桌面运行时包含四类线程 owner：

- Tauri main thread：顶层窗口、WebView、文件对话框和 desktop state。
- RenderWindowThread：winit event loop 与 Windows child HWND。
- RenderThread：`RenderLoop`、`TruvisRenderer`、`World` 与所有 Vulkan 对象。
- EditorServer thread：loopback HTTP/WebSocket 与 Web 静态文件。

关闭时先停止 Renderer 接收新的 desktop/editor 请求，再完成 Renderer、具体子系统、RenderRuntime 和 Vulkan 资源释放；
RenderThread 退出后才能销毁 child HWND，最后停止 EditorServer 并销毁 Tauri parent window。Server、WebView 和
Tauri main thread 在整个生命周期中都不得访问 `World` 或 Vulkan/VMA/WSI 对象。

EditorServer 默认只接受 loopback 地址并限制 Origin/CORS。若未来开放局域网访问，需要重新设计认证、授权和访问范围，
不能把当前本地进程内信任边界直接扩展到网络。

## 当前限制

- notification 不重放；恢复依赖查询、版本轮询或页面刷新。
- 多次查询不保证来自同一个原子 scene snapshot。
- 窗口最小化导致 Renderer frame 停止时，Editor 请求可能 timeout；当前没有独立 CPU request pump。
- embedded viewport 当前是 Windows child HWND 实现，native viewport 上方不能叠加 WebView DOM。
- HDRI GUI 只报告请求是否被 CPU scene 接受，不暴露最终 Loading/Ready/Failed 状态。
- 同一 canonical HDRI path 不强制 reload，首次异步失败后重新选择同一路径不会自动重试。

## 代码与操作入口

- 协议与 endpoint：`app/editor/bridge/src/`
- HTTP/WebSocket adapter：`app/editor/server/src/`
- RenderThread 协议适配：`app/truvis/src/editor_controller.rs`
- Tauri 本地命令桥：`app/truvis/src/desktop_command.rs`
- 开发、构建和运行参数：`app/editor/README.md`
