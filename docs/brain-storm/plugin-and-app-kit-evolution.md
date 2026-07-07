# Plugin 与 app-kit 演进

## 目标

把当前 App 手写组合的 GUI、camera/input、overlay 和 pipeline 能力，逐步收敛为可声明、可校验、可复用的 app 层 feature。
目标是减少隐式顺序依赖，而不是把 App 的业务编排权交给 runtime。

## 当前基线

当前 App / Plugin 生命周期、特有 render 能力显式调用和 app-kit 状态 owner 见
[`../summaries/runtime-app-plugin-boundaries.md`](../summaries/runtime-app-plugin-boundaries.md) 和
[`../summaries/frame-lifecycle.md`](../summaries/frame-lifecycle.md)。

## 待推进内容

- 设计 `PluginGroup`：支持注册标准 lifecycle plugin、声明 before / after / requires，并在启动期做拓扑校验。
- 定义 `PipelineFeature`：把设备能力检测、窗口尺寸资源、graph 贡献、debug image 输出和 shutdown 责任收敛为 app 层契约。
- 梳理 builtin feature：将 GUI、camera/input、overlay、pipeline controls 等 app-kit 能力拆成边界清楚的可选组件。
- 设计分层事件模型：区分 platform input/window 事件、app 业务事件和 world edit 意图；避免引入全局万能 event bus。
- 评估 app-kit 拆分：当 builtin feature 边界稳定后，再决定是否拆分 crate 或模块。

## 边界与非目标

- 不让 runtime 感知具体 plugin 类型、GUI 或 pipeline 策略。
- 不把所有特有 render 能力塞回统一 `Plugin` trait。
- 不强制一次性迁移所有 sample；允许主 App 和 sample 在过渡期继续手写组合。
- 不恢复独立 tick registry；后续调度优先通过 App hooks、标准 Plugin 和 `PluginGroup` 表达。

## 完成标准

- 至少一个 App 能通过 `PluginGroup` 注册并校验标准生命周期 plugin。
- pipeline feature 的初始化、resize、graph 贡献和 shutdown 顺序可由类型和依赖声明看出。
- GUI/input/overlay 的消费顺序仍由 App 策略明确控制。
- app-kit 中可复用 feature 的 owner、非职责和依赖方向能写入模块 README 或 summaries。
