# truvis

`truvis` 是 Truvis Editor 的 Tauri 应用壳和 RenderThread 产品 Client 提供者。它负责 Tauri
WebView、原生嵌入 viewport、`EditorIpc`、文件对话框和主线程关闭顺序；具体 GPU 渲染业务由
`truvis-renderer` 提供。

## 主要职责

- `desktop`：组装 Tauri/Tao main thread、WebView、frontend ports 与 `EmbeddedWinitHost`。
- `client`：在 RenderThread 上构建初始场景、处理 Editor 请求和本地桌面命令。
- `EditorIpc`：独占 Tauri invoke、event emit、两秒 request timeout 和 notification task。
- `main` / `build.rs` / Tauri 配置：提供桌面应用启动与打包入口。
- 场景转换工具位于根目录 [`scripts/scene/export_gltf.py`](../../scripts/scene/export_gltf.py)，不属于应用壳的运行时职责。

## 边界约束

- Tauri 主线程不直接访问 `GameWorld`；RenderThread Client 只通过 `RendererClient` 窄接口借用 CPU world。
- Client 不访问 Vulkan、RenderGraph 或具体 render pass。
- `EditorIpc` 不解释 Editor DTO；RenderThread Client 负责 DTO 到 World 的领域适配。
- 本地文件对话框返回的路径只通过 `DesktopCommandSender` 进入 RenderThread。
- parent window 必须晚于 Renderer/Runtime/Vulkan、child HWND 与 notification task 销毁。

渲染侧职责见 [`renderer/truvis-renderer/README.md`](../../renderer/truvis-renderer/README.md)。
