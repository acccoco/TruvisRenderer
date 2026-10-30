# truvis-render-thread

`truvis-render-thread` 是不依赖窗口 backend 的 OS 渲染线程宿主。它在窗口 owner 与唯一 `RenderLoop`
之间管理线程创建、输入/resize 转发、退出、完成通知、panic 传播和 join。

## 主要职责

- `RenderThread`：持有 `RenderThreadControl`、独立完成状态以及 OS `JoinHandle`。
- `RendererFactory`：只在目标 OS RenderThread 中构造一次具体 `Box<dyn Renderer>`。
- 初始化 factory：只捕获 backend 已验证可 Send 的具体平台句柄，在 RenderThread 内构造 `RenderThreadInit`。
- 完成握手：主执行结束后先请求退出、发布 Acquire/Release 完成标记，再通知窗口事件循环；
  执行与通知分别捕获 panic，join 优先返回原始执行异常，没有执行异常时才返回通知异常。
- `Drop` 只作为异常展开兜底，请求退出并 join；正常路径显式 `join`，以便窗口 owner 继续传播 panic payload。
  完成发布和 Drop 不调用日志后端，避免诊断阻断通知或在宿主展开中再次 panic。

## 生命周期契约

```text
window owner
  -> RenderThread::spawn(initial_size, build_init, renderer_factory, on_finished)
  -> OS RenderThread
       -> build_init(initial_size)
       -> renderer_factory()
       -> RenderLoop::run(control, init, renderer)
       -> publish finished
       -> on_finished()
  -> RenderThread::join()
  -> window drop
```

全部 Vulkan 对象和具体 Renderer 都只在 OS RenderThread 中创建、使用和销毁。窗口 owner 必须保证相关窗口在
`RenderThread::join` 返回前存活；本 crate 不持有窗口，也不解释平台句柄。

finished 只表示 init/factory/Loop 主执行已结束、可以进入 join；callback 可能仍在运行，
并不表示 OS thread 已返回。通知失败时，窗口可从 finished 查询进入 join，不自动重试
callback。spawn 成功只表示 OS thread 已创建，不表示 Renderer/窗口初始化成功。

当前偏差：Loop 异常可能跳过显式 GPU 回收。即使 join 已返回，也不能把错误返回当成
surface 已关闭的证明；完成分类与窗口销毁门槛仍需跨层处理。

## 依赖边界

```text
truvis-render-thread
  -> truvis-render-loop
```

本 crate 不直接依赖 winit、Windows API、Tauri、产品路径或具体 Renderer；RenderLoop 所需的退出、输入与 resize
契约继续由 `truvis-render-loop` 定义，线程完成状态和 panic 属于本 crate。
