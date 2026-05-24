# Plugin / Feature 演进方向

> 状态：活跃方向，更新于 2026-05-23。当前代码已有标准 `Plugin` 生命周期，
> 但尚未实现声明式插件容器和依赖调度。

本文合并 plugin、pass、GUI、platform、pipeline 和历史 tick 方案中仍有效的方向。

## 当前状态

- `RenderAppShell` 负责固定帧顺序，具体 App 通过 `RenderAppHooks` 接入 init / input / update / render / resize / shutdown。
- 标准 `Plugin` trait 覆盖 init / on_input / update / on_resize / shutdown。
- 具体能力仍由 App 以字段显式组合，例如 GUI、camera controller、overlay、RT pipeline、Shadertoy pipeline。
- GUI 的 Vulkan 后端能力是 `app-kit` 的私有实现细节，RenderGraph 适配与 UI 组合也在 app-kit / app 侧。
- RenderGraph pass 由 App 明确按顺序添加，当前不做自动 pipeline 策略选择。

## 演进目标

- 从 App 手写字段组合，逐步演进到 `PluginGroup` / feature registry。
- 插件声明 before / after / requires，启动期做拓扑校验，避免隐式顺序依赖。
- 将 camera/input、GUI overlay、pipeline controls、render pipeline 等能力拆成可装配 builtin plugin。
- 定义 pipeline feature 契约，让 raster / ray tracing / Shadertoy 可以并存、切换和 fallback。
- 事件按域分层，不引入全局万能 event bus。

## 推荐路线

### P0：文档化当前生命周期

- 明确 App hook 与 Plugin hook 的调用顺序、输入消费策略和 shutdown 反向遍历契约。
- 给现有 GUI、camera/input、overlay、RT pipeline 标注哪些是 App 策略，哪些是可复用 feature。
- 梳理 pass 的最小上下文需求，避免重新引入大上下文。

### P1：PluginGroup

- 引入插件集合类型，把当前 App 字段组合转换成显式注册。
- 支持依赖声明和顺序校验。
- 保留当前手写组合方式作为迁移过渡，不强制一次性改完全部 App。

### P2：PipelineFeature

- 定义渲染管线 feature 的能力检测、资源初始化、graph 贡献和 shutdown 契约。
- 将 RT、Shadertoy、Triangle/Raster 按同一策略接入。
- 引入 `PipelineManager` 负责 active pipeline 选择、设备能力 fallback 和热切换生命周期。

### P3：分层事件

- `PlatformEvent` 只表达窗口、输入、焦点、DPI、surface 状态。
- `WorldEvent` 只表达 CPU 语义层短生命周期事件。
- render-side 资源 ready / failed 仍通过明确 owner 和 frame 边界传递，不做跨层任意订阅。

## GUI 与输入边界

- GUI 输入抢占是 App 策略：当 GUI 想捕获鼠标或键盘时，App 决定是否继续交给 camera/input。
- GUI draw data 不进入通用 render ctx，仍由 GUI plugin 持有并在 render hook 贡献 pass。
- GUI texture id、font texture 和 render image id 应继续向单点定义收敛，避免 main/render 两侧重复常量。

## Tickable 草案的取舍

历史 `Tickable` 注册表方案的动机仍有效：camera/input 不应由 runtime 硬编码。
但当前主线已经有 App hooks 和 Plugin trait，因此后续应优先通过 builtin plugin / PluginGroup
解决，而不是恢复独立 `TickRegistry` API。

## 历史来源

本文提炼自以下归档文档：

- [`archive/plugin-pass-eventbus-evolution.md`](archive/plugin-pass-eventbus-evolution.md)
- [`archive/plugin-imgui-winit-multi-pipeline-integration.md`](archive/plugin-imgui-winit-multi-pipeline-integration.md)
- [`archive/app-tick-system.md`](archive/app-tick-system.md)
- [`archive/render-app-layering-analysis.md`](archive/render-app-layering-analysis.md)
