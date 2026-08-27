# Web Editor

`app/editor/` 是 Truvis 的 Tauri WebView 编辑器子系统。CPU `World` 仍是唯一权威场景状态；本目录只提供
协议、跨线程 endpoint 和可丢弃的 Web 投影，不维护第二份 scene、selection 或 material 权威副本。

## 目录职责

- `bridge/`：Rust 协议 DTO、每请求 oneshot reply，以及 Desktop/Renderer 两端的有界 channel endpoint。
- `web/`：Vite + React + TypeScript 页面；真实 backend 只通过 Tauri invoke/event 访问，开发模式可用 `?mock=1`
  进行不连接 Renderer 的视觉验收。
- `../truvis/src/editor_ipc.rs`：Tauri request、notification dispatcher、timeout 和 shutdown owner。
- `../truvis/src/editor_controller.rs`：Editor DTO 到 `World` API、SlotMap handle 和当前 selection 的唯一适配点。
- `../truvis/src/desktop_command.rs`：本地特权命令到 RenderThread 的私有有界桥，不属于通用 Editor DTO。

依赖方向固定为 `Tauri WebView -> truvis::editor_ipc -> bridge <- truvis::editor_controller -> truvis-world`。
Bridge 禁止依赖 Tauri、World、render runtime 或 GPU 类型。

## 协议与 IPC

Rust 协议类型定义在 `bridge/src/protocol/`，TypeScript 位于 `web/src/protocol/generated/`，由 `ts-rs`
生成，禁止手工修改。Bridge 通过 `create_editor_bridge` 返回 `DesktopEndpoint` 与 `AppEndpoint`：

- Tauri `editor_request` command 非阻塞写入容量为 `256` 的 request inbox，并等待请求自带的 oneshot reply。
- Renderer 每帧按数量和时间预算处理 Query/Command，只在 update 阶段访问 `World`。
- notification 通过容量为 `64` 的 outbox 交给 Tauri async dispatcher，再发送到 `main` WebView。
- `app/truvis/capabilities/main-editor.json` 只允许 `main` WebView listen/unlisten notification event。
- 页面保留一秒 `scene_version` 轮询，恢复刷新或事件丢失造成的投影失效。

## 构建与开发

从仓库根目录执行：

```powershell
just editor-web
just editor-web-dev
```

两个命令都会先从 Rust DTO 生成 TypeScript；`editor-web` 构建由 Tauri `frontendDist` 加载的 production 资源，
`editor-web-dev` 启动 Vite。独立浏览器只支持开发模式 `?mock=1`，不能访问真实 Renderer。

`Choose HDRI` 是独立的 Tauri-only 平台动作：完整 `PathBuf` 只经过 App-local queue 到 RenderThread，WebView
只接收文件名和 accepted/cancelled/error。accepted 不表示 decode、GPU upload 或 Alias distribution 已完成。

完整状态所有权、协议身份、一致性、背压和线程生命周期见
[`docs/summaries/editor-subsystem.md`](../../docs/summaries/editor-subsystem.md)。
