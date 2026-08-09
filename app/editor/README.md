# Web Editor

`app/editor/` 是 Truvis 的 Tauri WebView 编辑器子系统。CPU `World` 仍是唯一权威场景状态；本目录只提供
协议、跨线程 endpoint、网络适配和可丢弃的 Web 投影，不维护第二份 scene、selection 或 material 权威副本。

## 目录职责

- `bridge/`：Rust 协议 DTO、transport envelope，以及 Server/App 两端的有界 channel endpoint。
- `server/`：独立 OS 线程上的 Axum HTTP/WebSocket 服务，只负责 JSON、client 路由和静态文件。
- `web/`：Vite + React + TypeScript 页面；初次连接主动查询，之后通过 notification 和
  `scene_version` 轮询恢复投影。
- `../truvis/src/editor_controller.rs`：Editor 协议到 `World` API、SlotMap handle 和当前 selection 的唯一适配点。
- `../truvis/src/desktop_command.rs`：Tauri 本地特权命令到 RenderThread 的 App-local 有界桥，不属于 Editor 协议。

依赖方向固定为 `server -> bridge <- truvis::editor_controller -> truvis-world`。Bridge 与 Server 禁止依赖
World、render runtime 或 GPU 类型；desktop command 只存在于 `truvis` crate 内。

## 协议入口

Rust 协议类型定义在 `bridge/src/protocol/`，其中 `message.rs` 保存 Query/Command/Response/Notification，
`ids.rs` 保存 opaque Editor ID，`material.rs`、`scene.rs` 和 `selection.rs` 保存 DTO。Web 使用的 TypeScript
位于 `web/src/protocol/generated/`，由 `ts-rs` 从 Rust 类型生成，禁止手工修改。

Bridge 通过 `create_editor_bridge` 返回方向受限的 `ServerEndpoint` 与 `AppEndpoint`。Server 只增加
`ClientId` 路由信息，不重新定义 scene DTO，也不保存查询缓存。

## 构建与开发

从仓库根目录执行：

```powershell
just editor-web
just editor-web-dev
```

两个命令都会先安装或检查 npm 依赖，再从 Rust DTO 生成 TypeScript；`editor-web` 构建 production `dist/`，
`editor-web-dev` 启动 Vite 开发服务器。`just build-all`、`just truvis` 和 `just truvis-direct` 也会先执行
production Web 构建。Cargo `build.rs` 不调用 Node/npm，普通 `cargo check` 不隐式依赖 Web 工具链。

Vite dev 页面添加 `?mock=1` 可启用纯前端 mock transport；production build 不包含该入口。

## 运行参数

EditorServer 默认监听 `127.0.0.1:9473` 并提供 `app/editor/web/dist`：

- `TRUVIS_EDITOR_ADDR`：覆盖 loopback 监听地址。
- `TRUVIS_EDITOR_WEB_ROOT`：覆盖 production Web root。
- `GET /api/editor/v1/health`：存活检查。
- `GET /api/editor/v1/info`：协议版本与 WebSocket 地址信息。
- `WS /api/editor/v1/ws`：Editor Query/Command/Response/Notification 通道。

Truvis production WebView 通过 Tauri command 查询 EditorServer 实际监听地址。中央 `RenderViewport` 只提交
DOM slot 的物理像素矩形给 child HWND 宿主；scene/material 协议仍只走 WebSocket，viewport 输入直接由原生窗口接收。

## Tauri 本地能力

`Choose HDRI` 是独立的 Tauri-only 平台动作：Rust command 打开 `.hdr`/`.exr` 文件选择器，完整 `PathBuf`
只经过 App-local queue 到 RenderThread，Web 只接收文件名和 accepted/cancelled/error。浏览器开发与 mock 模式
禁用该入口。

accepted 只表示 `World` 已接受天空 texture 请求，不表示 HDR/EXR decode、GPU upload 或 Alias distribution 已完成，
因此 UI 使用 `HDRI requested`。同一 canonical path 不强制 reload；首次异步失败后再次选择同一路径不会自动重试。

完整状态所有权、协议身份、一致性、背压和线程生命周期见
[`docs/summaries/editor-subsystem.md`](../../docs/summaries/editor-subsystem.md)。
