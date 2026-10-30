# truvis-render-loop

`truvis-render-loop` 定义 Renderer 框架层的契约、统一帧执行器和最小跨线程控制契约。平台无关的
`RenderLoop` 同时拥有具体 Renderer、`RenderRuntime` 和完整渲染循环，供平台入口和具体 renderer state 共同使用。

## 主要职责

- `RenderLoop`：非泛型的唯一完整帧执行器，持有 `RenderRuntime`、输入队列和 `Box<dyn Renderer>`，仅公开 `run` 入口。
- `Renderer`：`RenderLoop` 在 init / input / update / after_prepare / render / resize / shutdown 阶段回调具体 Renderer 的 object-safe 契约。
- `RendererInitCtx` / `RendererResizeCtx` / `RendererShutdownCtx`：包装对应 runtime 阶段能力并提供 Renderer 需要的窗口元数据。
- `InputEvent`：平台输入事件的引擎侧表示
- `RenderThreadControl` / `RenderThreadInit`：帧执行器消费的退出、输入、resize 和窗口初始化契约；线程完成与 panic 归属于
  `truvis-render-thread`。

## 设计意图

- `RenderLoop::run(control, init, renderer)` 在内部统一执行初始化、完整帧循环和 shutdown，保证所有具体 Renderer 都经过同一个帧骨架。
- Renderer factory 在 OS RenderThread 内创建 `Box<dyn Renderer>`，随后由 `truvis-render-thread::RenderThread` 调用统一 RenderLoop
  入口；winit 窗口层不知道具体 Renderer、内部子系统或 `RenderRuntime`。
- RenderLoop 定义阶段边界；具体 Renderer 通过 `Renderer` 暴露固定 hook 点，并自行持有、编排 ImGui、camera/input、overlay 和具体渲染子系统。
- `after_prepare` 是 Renderer 可选同步查询点，发生在 runtime prepare 完成后、render graph 组图前，用于调用同步 raycast 等依赖 GPU scene 快照的接口。
- `SubsystemLifecycle` 属于 `renderer-kit`；具体 subsystem 属于 `renderer-imgui`、`renderer-rendering` 或各自 Renderer，本 crate 不知道它们的类型、顺序或数量。
- GUI frame 构建、pass 贡献、overlay 和输入消费均通过具体类型由 Renderer 显式调用，不进入 `RenderLoop`。
- winit backend 只负责窗口、事件循环和事件适配；本 crate 不依赖 `winit`，也不反向依赖线程宿主。

上传确认、主视图历史确认与帧号推进是不同边界：prepare submit 返回后确认 GPU 数据
版本，render 返回后按其 bool 确认实例历史，present 或无 image 的空 signal submit
正常返回后推进帧号。零尺寸暂停发生在 begin_frame 之前，不进入上述确认。

正常退出直接 wait_idle → Renderer shutdown → Runtime destroy，不插入额外日志调用。
阶段内 panic 仍可能跳过后续关闭；RenderThread finished 不能证明 surface/root 已回收。
详见[帧执行与关闭约束](../../../docs/design/frame-execution-and-window-lifecycle.md)。

原生窗口通过 `RenderThreadControl::publish_resize` 同时发布尺寸 generation 和 `InputEvent::Resized`。
即使窗口恢复原尺寸，也会在下一次 update 前清理旧交互；DLSS 内部 resize 不走该窗口通道。

窗口 DPI 通过 `InputEvent::ScaleFactorChanged` 传递，Loop 截获并只保留最终值，
普通输入仍保持 FIFO。恢复渲染前在 GPU idle 安全点调用 `Renderer::on_scale_factor_changed`；
同值不回调，物理尺寸不变也可交付，零尺寸暂停期间不重建字体资源。

退出标记同时限制输入生产和排空：send_input 观察到退出后不再入队，Loop 在 drain
期间检查退出，并在 drain 后重查，避免积压拖延关闭或再开始 resize/frame。与退出
并发且已越过检查的投递仍可能入队；这是停止接收的观察边界，不是事务式关闭通道。
正常暂停/零尺寸期间仍使用无界 FIFO，不合并或丢弃松键/焦点等状态事件。

## Ctx 边界

- `RendererInitCtx` 包装 `RenderRuntimeInitCtx` 并附带窗口 size / scale factor；Renderer 直接用 `&mut ctx.runtime`
  初始化自己持有的长期资源。
- `RenderRuntimeUpdateCtx` 面向 CPU 更新，提供 `GameWorld`、只读帧状态/计时与内部尺寸请求入口；DLSS 配置由 Renderer 持有。
- `RenderRuntimeRayCastCtx` 只在 Renderer `after_prepare` hook 中出现。
- `RenderRuntimeRenderCtx` 面向渲染录制；具体 Renderer 可在 `renderer-kit` 内进一步裁剪为 `SubsystemRenderCtx`，该类型不属于 render-loop 层。
- `RendererResizeCtx` 和 `RendererShutdownCtx` 分别包装对应 runtime ctx；Renderer 直接用它们重建或释放自有子系统资源。
  manager-owned image/view 必须通过 `GfxResourceRegistry` 释放，shader-visible view 必须通过 `ShaderBindingSystem` 注销。

## 边界约束

- 不创建平台窗口，不处理 winit lifecycle，不持有具体 Renderer / 子系统业务状态
- `RenderLoop` 是唯一完整帧骨架，不提供运行时 Renderer 替换或注册表
- 不定义子系统 trait、visitor 或子系统生命周期调度；这些属于具体 Renderer 的阶段内部编排
- GUI draw data 不进入 Renderer 或 runtime ctx，由 `renderer-imgui` 的 `ImGuiSubsystem` 自行管理
