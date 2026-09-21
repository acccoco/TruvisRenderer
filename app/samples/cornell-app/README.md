# cornell-app

`rt-cornell` 是使用完整 `TruvisRenderer` 的 standalone 入口。它与 Truvis Tauri App 使用相同的 realtime/offline、ImGui、相机交互、拾取、描边和坐标轴能力，但不创建 Tauri、WebView、Editor 通信队列或文件对话框。

两个 App 都默认 Manual，并共用 [`truvis-scenes`](../../truvis-scenes/README.md) 的场景与启动参数：

```text
just cornell
just cornell --scene cornell
just cornell imgui --scene sponza
cargo run --bin rt-cornell -- --scene manual
```

`CornellApp::run` 在窗口创建前解析参数，随后将 factory 交给 `StandaloneWinitHost`。factory 在 RenderThread 内构造 `CornellAppClient` 和 Renderer。Client 只持有共享 `SceneInitializer`，无需处理 selection 通知或关闭 receiver；Renderer 仍完整维护选中状态与 GPU 生命周期。

关闭时沿用宿主顺序：结束 RenderThread、回收 Renderer/Runtime/Vulkan，再销毁窗口。`imgui` 启动选项仅控制 Streamline 调试 UI，`no-validation` 沿用公共脚本规则。
