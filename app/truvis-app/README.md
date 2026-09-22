# truvis-app

`truvis-app` 是 Truvis Editor 的 Tauri 应用壳和 RenderThread 产品 Client 提供者。它负责 Tauri
WebView、原生嵌入 viewport、`EditorIpc`、文件对话框和主线程关闭顺序；具体 GPU 渲染业务由
`truvis-renderer` 提供。

## 主要职责

- `desktop`：组装 Tauri/Tao main thread、WebView、frontend ports 与 `EmbeddedWinitHost`。
- `client`：在 RenderThread 上组合共享 `truvis-scenes::SceneInitializer`，处理 Editor 请求和本地桌面命令。
- `EditorIpc`：独占 Tauri invoke、event emit、两秒 request timeout 和 notification task。
- `mcp_server`：始终启动固定 loopback Streamable HTTP MCP；工具请求只进入
  有界 Automation endpoint，由 RenderThread 的 `WorldAutomationController` 修改 CPU scene。
- `main` / `build.rs` / Tauri 配置：提供桌面应用启动与打包入口。
- 场景转换工具位于根目录 [`scripts/scene/export_gltf.py`](../../scripts/scene/export_gltf.py)，不属于应用壳的运行时职责。

默认 Manual，支持 `--scene manual|sponza|cornell`。启动参数与场景定义由 [`truvis-scenes`](../truvis-scenes/README.md) 统一维护，帮助和非法参数在窗口创建前结束。

## 边界约束

- Tauri 主线程不直接访问 `GameWorld`；RenderThread Client 只通过 `RendererClient` 窄接口借用 CPU world。
- Client 不访问 Vulkan、RenderGraph 或具体 render pass。
- `EditorIpc` 不解释 Editor DTO；RenderThread Client 负责 DTO 到 World 的领域适配。
- 本地文件对话框返回的路径只通过 `DesktopCommandSender` 进入 RenderThread。
- parent window 必须晚于 Renderer/Runtime/Vulkan、child HWND 与 notification task 销毁。

## MCP Automation

MCP server 固定监听 `127.0.0.1:8765/mcp`，不使用 token 或其他身份认证；访问控制依赖 loopback
地址和固定 Host 校验。RMCP 支持 MCP `2025-06-18`、`2025-11-25` 和 `2026-07-28` 的 stateless
Streamable HTTP；2025 版本通过 `initialize` 协商，2026 版本可以使用 `server/discover` 和现代 request
metadata，标准 `MCP-Protocol-Version` header 由 RMCP 校验。端口被占用时 Truvis 启动失败，用户需要释放
`127.0.0.1:8765` 后重新启动；不提供旧 HTTP+SSE、REST 别名、自动换端口、远程监听或 CORS/OAuth
设施。本地其他进程可以访问该 endpoint。协议版本只影响 MCP envelope 和生命周期，不影响 Automation
wire DTO。

第一阶段工具只操作 CPU scene：查询 summary、objects、instance、material，更新 transform/material，
发起 scene import 和查询 import status。返回的 scene version 只表示 RenderThread 已接受的 CPU mutation，
不表示 asset ready、GPU visible 或最终像素完成。Renderer settings、capture 和 Lua 属于后续阶段。

渲染侧职责见 [`renderer/truvis-renderer/README.md`](../../renderer/truvis-renderer/README.md)。
