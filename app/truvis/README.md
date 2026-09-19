# truvis

`truvis` 是 Truvis Editor 的 Tauri 应用壳。它负责 Tauri WebView、原生嵌入 viewport、
`EditorIpc`、文件对话框和主线程关闭顺序；具体渲染业务由 `truvis-renderer` 提供。

## 主要职责

- `desktop`：组装 Tauri/Tao main thread、WebView、frontend ports 与 `EmbeddedWinitHost`。
- `EditorIpc`：独占 Tauri invoke、event emit、两秒 request timeout 和 notification task。
- `main` / `build.rs` / Tauri 配置：提供桌面应用启动与打包入口。
- `scripts/prepare_bistro.py`：离线把 ORCA Bistro FBX 与 DDS 贴图转换为 glTF 和外部 PNG；
  不生成独立 manifest 或相机/灯光 JSON，应用运行时继续通过现有 glTF 导入链路读取资产。

## 边界约束

- 不直接访问 `GameWorld`、Vulkan、RenderGraph 或具体 render pass。
- 不解释 Editor DTO；只把请求和通知适配到 Tauri invoke/event。
- 本地文件对话框返回的路径只通过 `DesktopCommandSender` 进入 RenderThread。
- parent window 必须晚于 Renderer/Runtime/Vulkan、child HWND 与 notification task 销毁。

渲染侧职责见 [`renderer/truvis-renderer/README.md`](../../renderer/truvis-renderer/README.md)。
