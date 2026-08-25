# truvis-winit-app

`truvis-winit-app` 是平台入口层，负责 standalone 顶层窗口或 Windows child HWND 的创建、winit 事件循环与渲染线程启动。

## 主要职责

- 创建并管理 winit `EventLoop` 与窗口生命周期
- 通过 `EmbeddedWinitHost` 在 `RenderWindowThread` 创建 parent-owned child HWND
- 将平台事件转换为引擎输入事件并转发
- 通过两种窗口模式共用的 `RenderThread` handle 管理渲染线程，并在线程内创建具体 App、进入唯一 `RenderAppRunner::run`
- 提供 `SendWrapper<T>`，在窗口存活不变量成立时受控跨线程交接 raw handle 初始化参数

## 入口位置

- `src/app.rs`：平台运行时封装
- `src/embedded.rs`：Windows child HWND 宿主、几何命令与窗口线程生命周期
- `src/render_thread.rs`：standalone / embedded 共用的 `RenderThread` owner
- `src/winit_event_adapter.rs`：winit 事件到 `InputEvent` 的转换

具体 app 与 sample 入口位于 `app/` 下，本 crate 不声明可执行入口。

## 启动方式

- 入口：`WinitApp::run_app(|| Box<dyn RenderApp>)`
- 示例：`WinitApp::run_app(|| Box::new(DemoState::default()))`
- 嵌入入口：`EmbeddedWinitHost::spawn(parent_raw_handle, || Box<dyn RenderApp>)`

## 线程模型

- standalone 模式由 main thread 持有 winit `EventLoop` 和顶层 `Window`。
- embedded 模式由 Tauri/Tao 占用 main thread，专用 `RenderWindowThread` 持有 winit `EventLoop` 和 child `Window`。
- embedded startup 先把 `EventLoopProxy` 交还 Tauri setup，再异步创建 child；不能让 main thread 同步等待跨线程
  `CreateWindowEx`，否则 Windows 的 parent notification 会造成互等。创建前的 viewport rect 会缓存 latest 值。
- child 创建后保持隐藏的 `1x1` 初始状态，直到收到第一个非零 DOM viewport rect；窗口线程先应用该 rect，再从
  `Window::inner_size` 创建 `RenderThread`。因此 App、GUI plugin 与 Vulkan swapchain 共享同一个真实初始 extent，
  不依赖后续 resize 才完成初始化。
- 窗口 owner 通过 `RenderThread` 的 `spawn`、`request_exit`、`publish_resize`、`send_input`、`is_finished` 和 `join` 管理线程，不直接操作共享原子状态。
- OS RenderThread 内先解包 `SendWrapper<RenderThreadInit>`，再执行 factory 得到 `Box<dyn RenderApp>`，随后进入 `RenderAppRunner::run`；所有 Vulkan 对象都在该线程创建、使用和销毁。
- embedded render child 收到任意鼠标按下事件时，由 `RenderWindowThread` 显式调用 `SetFocus` 取得 keyboard focus；
  点击周围 WebView 控件后由 WebView 自然收回焦点。两侧之间不转发或复制键盘事件。
- 输入事件通过 `RenderThread::send_input` 和 `RenderThreadControl` 持有的无界 channel 传给 Runner，再进入当前帧待处理队列。
- resize 使用 latest-size 模式合并连续事件；零尺寸窗口不会触发 swapchain 重建。
- 退出时窗口 owner 发出退出信号；OS RenderThread 完成 Runner 内部 shutdown 和 GPU 资源释放后，窗口 owner 再 join
  渲染线程并允许 `Window` drop。embedded 模式还会在 child HWND drop 后 join `RenderWindowThread`，保证 parent HWND 最后销毁。

## 模块边界

- 本模块不实现具体渲染算法，只负责平台与线程编排。
- 本模块不依赖 Tauri、DOM、WebView 或 editor 协议；嵌入 API 只接收 parent raw handle 和物理像素矩形。
- 本模块不依赖主体 app 或 samples，调用方通过 `WinitApp::run_app` 注入 `Box<dyn RenderApp>`。
- App / Plugin 契约、统一帧执行器与最小线程控制契约定义在 `engine/app-frame/truvis-app-frame`。
- 渲染运行时在 `engine/render/truvis-render-runtime`，具体 app 复用的 RT / 后处理 pass 在 `app/app-render-passes`。
