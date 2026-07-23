# Web Editor

`app/editor/` 是 Truvis 的 Tauri WebView 编辑器子系统。CPU `World` 仍是唯一权威场景状态；本目录只提供通信契约、网络适配和
可丢弃的 Web 投影，不维护第二份 scene、selection 或 material 权威副本。

## 目录职责

- `bridge/`：Rust 协议 DTO、跨线程 envelope，以及 Server/App 两端的有界 channel endpoint。
- `server/`：独立 OS 线程上的 Axum HTTP / WebSocket 服务，只负责 JSON、client 路由和静态文件。
- `web/`：Vite + React + TypeScript 页面；初次连接主动查询，之后通过通知和 `scene_version` 轮询恢复投影。
- `../truvis/src/editor_controller.rs`：Editor 协议到 `World` API、SlotMap handle 和当前 selection 的唯一适配点。
- `../truvis/src/desktop_command.rs`：Tauri 本地特权命令到 RenderThread 的 App-local 有界桥，不属于 Editor 协议。

依赖方向固定为 `server -> bridge <- truvis::editor_controller -> truvis-world`。Bridge 与 Server 禁止依赖 World、
render runtime 或 GPU 类型。`desktop_command` 只存在于 `truvis` crate 内，不向 Bridge/Server 增加依赖。

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

Truvis 运行后，production `dist/` 直接作为 Tauri WebView 内容加载；WebView 通过 Tauri command 查询 EditorServer 的
实际 WebSocket 地址，默认是 `ws://127.0.0.1:9473/api/editor/v1/ws`。中央 `RenderViewport` 只把 DOM 计算出的物理像素
矩形提交给 child HWND 宿主，Editor 场景与材质协议仍只走现有 WebSocket。`Choose HDRI` 是独立的 Tauri-only
平台动作：Rust command 打开 `.hdr` / `.exr` 原生文件选择器，完整路径只经过 App-local queue 到 RenderThread，Web
只接收文件名和 `cancelled / accepted / error`。浏览器开发与 mock 模式禁用该按钮并显示 desktop-only 提示。

`accepted` 仅表示 `World` 已接受天空 texture 请求，不表示 HDR/EXR decode、GPU upload 或 Alias distribution 已完成，
因此界面使用 `HDRI requested` 而不是 `Loaded`。同一 canonical path 不强制 reload；首次异步失败后再次选择同一路径
不会重试，这是当前 v1 限制。

Web UI 使用浅色主题，并直接复用仓库级 `assets/resources/DruvisIII.png`；Tauri 的 Windows EXE / 窗口图标使用同一
源图机械转换出的 `app/truvis/icons/DruvisIII.ico`，不维护另一套自研品牌图标。`EditorWorkspace` 持有左右面板宽度与边界拖拽状态；
拖动任一竖向分隔线只改变 DOM grid，中央 slot 的 `ResizeObserver` 随后沿既有 command 更新 child HWND。桌面宽度下中央
viewport 保留最小宽度，分隔线也支持键盘方向键调整；窄屏浏览器布局继续切换为两列或纵向排列并隐藏分隔线。

完整设计、协议语义、背压策略和已知限制见 [`docs/editor.md`](../../docs/editor.md)。
