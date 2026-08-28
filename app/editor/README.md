# Web Editor

`app/editor/web` 是 Truvis 的 React/TypeScript Tauri WebView 页面。CPU `World` 仍是唯一权威场景状态；
Web 只保存可丢弃的 scene、selection 和 material 投影。

## 边界

- `renderer/editor/bridge`：Rust DTO、每请求 oneshot reply，以及 Frontend/Renderer 两端的有界 channel。
- `app/editor/web`：Vite + React + TypeScript 页面；真实 backend 只通过 Tauri invoke/event 访问。
- `app/truvis/src/editor_ipc.rs`：Tauri request、notification dispatcher、两秒 timeout 和 shutdown owner。
- `renderer/truvis/src/editor_controller.rs`：Editor DTO 到 `World` API 和 SlotMap handle 的适配器。
- `renderer/truvis/src/desktop_command.rs`：本地特权命令的渲染侧消费者，不属于通用 Editor DTO。

```text
Tauri WebView
  -> truvis_app::editor_ipc
       -> TruvisFrontendPorts
            <bounded in-process channels>
       -> TruvisRendererPorts
            -> EditorController / DesktopCommandController
                 -> World
```

Bridge 和 Renderer 层都禁止依赖 Tauri。`EditorIpc` 不解释领域请求；controller 不实现
`SubsystemLifecycle`。

## 协议与 IPC

Rust 协议类型定义在 `renderer/editor/bridge/src/protocol/`，TypeScript 位于
`app/editor/web/src/protocol/generated/`，由 `ts-rs` 生成，禁止手工修改。`create_editor_bridge` 返回
`FrontendEndpoint` 与 `RendererEndpoint`：

- request inbox 容量为 `256`，每个 request 携带独立 oneshot reply。
- Renderer 每帧最多处理 `32` 条、最多使用 `500 μs`，只在 update 阶段访问 `World`。
- notification outbox 容量为 `64`，队列满时允许丢弃。
- `app/truvis/capabilities/main-editor.json` 只允许 `main` WebView listen/unlisten notification event。
- 页面保留一秒 `scene_version` 轮询，以恢复刷新或事件丢失造成的投影失效。

`Choose HDRI` 是 Tauri-only 平台动作：完整 `PathBuf` 只经过 App/Renderer 之间的私有队列，WebView
只接收文件名和 accepted/cancelled/error。accepted 不表示 decode、GPU upload 或 Alias distribution 已完成。

## 构建与开发

从仓库根目录执行：

```powershell
just editor-web
just editor-web-dev
```

两个命令都会先从 Rust DTO 生成 TypeScript。`editor-web` 构建 Tauri `frontendDist` 加载的生产资源；
`editor-web-dev` 启动 Vite。独立浏览器只支持 `?mock=1`，不访问真实 Renderer。

完整所有权、背压和关闭顺序见
[`docs/summaries/editor-subsystem.md`](../../docs/summaries/editor-subsystem.md)。
