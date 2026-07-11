# Web Editor

`app/editor/` 是 Truvis 的浏览器编辑器子系统。CPU `World` 仍是唯一权威场景状态；本目录只提供通信契约、网络适配和
可丢弃的 Web 投影，不维护第二份 scene、selection 或 material 权威副本。

## 目录职责

- `bridge/`：Rust 协议 DTO、跨线程 envelope，以及 Server/App 两端的有界 channel endpoint。
- `server/`：独立 OS 线程上的 Axum HTTP / WebSocket 服务，只负责 JSON、client 路由和静态文件。
- `web/`：Vite + React + TypeScript 页面；初次连接主动查询，之后通过通知和 `scene_version` 轮询恢复投影。
- `../truvis/src/editor_controller.rs`：协议到 `World` API、SlotMap handle 和当前 selection 的唯一适配点。

依赖方向固定为 `server -> bridge <- truvis::editor_controller -> truvis-world`。Bridge 与 Server 禁止依赖 World、
render runtime 或 GPU 类型。

## 开发命令

从仓库根目录构建或启动 Web：

```powershell
just editor-web
just editor-web-dev
```

两个命令都会先安装/检查 npm 依赖，并在构建或启动 Vite 前从 Rust DTO 生成 TypeScript 协议；协议导出是 `justfile`
内部步骤，不需要单独调用。`just build-all`、`just truvis` 和 `just truvis-direct` 也会先执行生产前端构建。

开发服务器页面添加 `?mock=1` 可以启用纯前端 mock transport；production build 不启用该路径。Truvis Server 默认监听
`127.0.0.1:9473` 并提供 `app/editor/web/dist`，可用 `TRUVIS_EDITOR_ADDR` 和 `TRUVIS_EDITOR_WEB_ROOT` 覆盖。

Truvis 运行后，正式页面入口是 Editor Server 的实际绑定地址，默认是 `http://127.0.0.1:9473/`。`Truvis Overlay`
主面板顶部会显示该地址和 `Open Web Editor` 按钮；按钮使用系统默认浏览器打开正式入口，不依赖当前 selection，也不会
自动切换到 Vite 的 `5173` 开发端口。

完整设计、协议语义、背压策略和已知限制见 [`docs/editor.md`](../../docs/editor.md)。
