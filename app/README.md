# app

`app/` 只保存面向最终用户的应用启动壳。它依赖 `renderer/` 提供的具体 Renderer，并可直接使用
`engine/e60-platform` 的窗口宿主；渲染 pass、Subsystem、shader 与渲染侧通信 controller 不属于本层。

## 目录职责

- [`truvis/`](truvis/README.md)：Tauri Editor 壳，拥有 WebView、`EditorIpc`、文件对话框、嵌入 viewport 与关闭顺序。
- [`editor/web/`](editor/web/)：React / TypeScript Editor 页面及 Tauri transport adapter。
- `samples/hello-triangle/`：`triangle` 的薄 standalone 启动入口。
- `samples/shader-toy/`：`shader-toy` 的薄 standalone 启动入口。
- `samples/cornell/`：`rt-cornell` 的薄 standalone 启动入口。

## 边界约束

- Tauri API、`AppHandle`、invoke/event、dialog 和两秒 request timeout 只存在于 `app/truvis`。
- standalone sample 只初始化日志、图标和窗口参数，并把 Renderer factory 交给 `StandaloneWinitHost`。
- App 不直接访问 `World`、Vulkan、RenderGraph、具体 pass 或 Renderer subsystem。
- 主体 Tauri parent window 必须晚于 Renderer/Runtime/Vulkan、child HWND 与 notification task 销毁。

具体渲染能力见 [`renderer/README.md`](../renderer/README.md)。
