# truvis-winit-host

`truvis-winit-host` 是 winit 窗口 backend，负责 standalone 顶层窗口和 Windows embedded child HWND；
OS 渲染线程生命周期由独立的 `truvis-render-thread` 管理。

## 主要职责

- `StandaloneWinitHost`：在 main thread 管理顶层 winit `EventLoop`、`Window` 与关联的 `RenderThread`。
- `StandaloneWindowOptions`：接收 app 入口提供的标题、逻辑尺寸、透明策略和可选图标内容。
- `EmbeddedWinitHost`：向外层桌面壳提供 viewport rect、关闭请求与 `RenderWindowThread` 生命周期句柄。
- `EmbeddedWinitHandler`：在窗口线程持有 child HWND、事件循环与 `RenderThread`。
- `WinitInputAdapter`：把 winit 事件转换为 render-loop 层定义的 `InputEvent`。
- `Win32RenderSurface`：只跨线程传递可 Send 的 `Win32WindowHandle` 和 `WindowsDisplayHandle`，并在目标 RenderThread 内重建 raw handle。

## 入口

```rust
StandaloneWinitHost::run(window_options, || Box::new(DemoRenderer::default()));
EmbeddedWinitHost::spawn(parent_raw_handle, || Box::new(DemoRenderer::default()))?;
```

standalone 的日志初始化、图标资源路径和窗口外观由具体 app 入口决定；本 crate 不依赖产品路径、Tauri、DOM、WebView 或 editor 协议。

## 线程与窗口生命周期

- standalone 模式由 main thread 持有 winit `EventLoop` 和顶层 `Window`。
- embedded 模式由外层桌面 runtime 持有 main thread，专用 `RenderWindowThread` 持有 winit `EventLoop` 和 child `Window`。
- embedded startup 先把 `EventLoopProxy` 交还调用方，再异步创建 child，避免跨线程 parent notification 阻塞 main-thread 消息泵。
- child 初始保持隐藏的 `1x1`；首次非零 viewport rect 先通过 `SetWindowPos` 设置真实尺寸，再启动 RenderThread。
- viewport 几何只属于窗口层；输入通过 `InputEvent` 进入 `RenderLoop`，resize 继续使用 latest-size、generation 和 debounce。
- render child 收到鼠标按下后由窗口线程执行 `SetFocus`；点击 WebView 后焦点按 Windows 默认行为返回，不复制键盘事件。
- 窗口 owner 正常路径先等待 `RenderThread::join` 完成，再销毁 `Window`；字段 drop 顺序与 RenderThread fallback
  同时覆盖异常展开，embedded 入口随后 join `RenderWindowThread`，保证 parent HWND 最后释放。

## 依赖边界

```text
truvis-winit-host
  -> truvis-render-thread
  -> truvis-render-loop
```

`RenderLoop` 和 `Renderer` 阶段契约属于 `truvis-render-loop`；具体 Renderer、子系统、日志、产品标题、图标路径与渲染管线属于 `app/`。
