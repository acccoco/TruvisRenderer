# app

`app/` 保存面向最终用户的应用启动壳和产品业务 Client。它依赖 `renderer/` 提供的具体 Renderer，并可直接使用
`engine/e60-platform` 的窗口宿主；渲染 pass、Subsystem、shader 与 GPU 生命周期不属于本层。

## 目录职责

- [`truvis-app/`](truvis-app/README.md)：Tauri Editor 壳，拥有 WebView、`EditorIpc`、文件对话框、嵌入 viewport 与关闭顺序。
- [`editor/web/`](editor/web/)：React / TypeScript Editor 页面及 Tauri transport adapter。
- `samples/triangle-app/`：`triangle` 的薄 standalone 启动入口。
- `samples/shader-toy-app/`：`shader-toy-renderer` 的薄 standalone 启动入口。
- [`samples/cornell-app/`](samples/cornell-app/README.md)：`rt-cornell` 的 standalone 入口，使用完整 `TruvisRenderer`。
- [`truvis-scenes/`](truvis-scenes/README.md)：无 Tauri 依赖的共享场景定义、导入状态和启动参数。
- `truvis-app/src/client/`：由 App 提供、在 RenderThread 上运行的场景与 Editor 业务 Client。

## 边界约束

- Tauri API、`AppHandle`、invoke/event、dialog 和两秒 request timeout 只存在于 `truvis-app`。
- standalone sample 负责日志、图标、窗口参数和启动选择，并把 Renderer factory 交给 `StandaloneWinitHost`；场景由 RenderThread Client 构建。
- Tauri 主线程和 WebView 不直接访问 `GameWorld`、Vulkan、RenderGraph、具体 pass 或 Renderer subsystem；
  RenderThread 上的 App Client 只能通过 RendererClient 窄接口借用 CPU `GameWorld`。
- 主体 Tauri parent window 必须晚于 Renderer/Runtime/Vulkan、child HWND 与 notification task 销毁。

Truvis 与 Cornell 两个入口都默认 Manual，均支持 `--scene manual|sponza|cornell`。两者使用相同渲染与交互能力，区别是 standalone 窗口与 Tauri/Web Editor 配套设施。

具体渲染能力见 [`renderer/README.md`](../renderer/README.md)。
