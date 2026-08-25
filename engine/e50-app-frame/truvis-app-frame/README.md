# truvis-app-frame

`truvis-app-frame` 定义 App 框架层的契约、统一帧执行器和最小跨线程控制契约。平台无关的
`RenderAppRunner` 同时拥有具体 App、`RenderRuntime` 和完整渲染循环，供平台入口和具体 app state 共同使用。

## 主要职责

- `RenderAppRunner`：非泛型的唯一完整帧执行器，持有 `RenderRuntime`、输入队列和 `Box<dyn RenderApp>`，仅公开 `run` 入口。
- `RenderApp`：`RenderAppRunner` 在 init / input / update / after_prepare / render / resize / shutdown 阶段回调具体 App 的 object-safe 契约。
- `RenderAppInitCtx` / `RenderAppResizeCtx` / `RenderAppShutdownCtx`：包装对应 runtime 阶段能力并提供 App 需要的窗口元数据。
- `InputEvent`：平台输入事件的引擎侧表示
- `RenderThreadControl` / `RenderThreadInit`：帧执行器消费的退出、输入、resize 和窗口初始化契约；线程完成与 panic 归属于
  `truvis-render-thread`。

## 设计意图

- `RenderAppRunner::run(control, init, app)` 在内部统一执行初始化、完整帧循环和 shutdown，保证所有具体 App 都经过同一个帧骨架。
- App factory 在 OS RenderThread 内创建 `Box<dyn RenderApp>`，随后由 `truvis-render-thread::RenderThread` 调用统一 Runner
  入口；winit 窗口层不知道具体 App、内部子系统或 `RenderRuntime`。
- Runner 定义阶段边界；具体 App 通过 `RenderApp` 暴露固定 hook 点，并自行持有、编排 ImGui、camera/input、overlay 和具体渲染子系统。
- `after_prepare` 是 App 可选同步查询点，发生在 runtime prepare 完成后、render graph 组图前，用于调用同步 raycast 等依赖 GPU scene 快照的接口。
- `SubsystemLifecycle` 属于 `app-kit`；具体 subsystem 属于 `app-imgui`、`app-rendering` 或各自 App，本 crate 不知道它们的类型、顺序或数量。
- GUI frame 构建、pass 贡献、overlay 和输入消费均通过具体类型由 App 显式调用，不进入 `RenderAppRunner`。
- winit backend 只负责窗口、事件循环和事件适配；本 crate 不依赖 `winit`，也不反向依赖线程宿主。

## Ctx 边界

- `RenderAppInitCtx` 包装 `RenderRuntimeInitCtx` 并附带窗口 size / scale factor；App 直接用 `&mut ctx.runtime`
  初始化自己持有的长期资源。
- `RenderRuntimeUpdateCtx` 面向 CPU 更新，提供 `World`、帧设置和 `DlssOptions`，不承担 command recording。
- `RenderRuntimeRayCastCtx` 只在 App `after_prepare` hook 中出现。
- `RenderRuntimeRenderCtx` 面向渲染录制；具体 App 可在 `app-kit` 内进一步裁剪为 `SubsystemRenderCtx`，该类型不属于 frame 层。
- `RenderAppResizeCtx` 和 `RenderAppShutdownCtx` 分别包装对应 runtime ctx；App 直接用它们重建或释放自有子系统资源。
  manager-owned image/view 必须通过 `GfxResourceManager` 释放，shader-visible view 必须通过 `ShaderBindingSystem` 注销。

## 边界约束

- 不创建平台窗口，不处理 winit lifecycle，不持有具体 App / 子系统业务状态
- `RenderAppRunner` 是唯一完整帧骨架，不提供运行时 App 替换或注册表
- 不定义子系统 trait、visitor 或子系统生命周期调度；这些属于具体 App 的阶段内部编排
- GUI draw data 不进入 frame 或 runtime ctx，由 `app-imgui` 的 `ImGuiSubsystem` 自行管理
