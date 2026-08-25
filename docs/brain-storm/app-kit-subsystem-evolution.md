# App-owned 子系统与 app-kit 演进

## 目标

继续收敛 GUI、camera/input、overlay 和 pipeline 的 app 层边界，让可选能力通过具体 App 字段与调用点
进行编译期静态装配，同时保持资源 owner、生命周期顺序与 RenderGraph 编排显式可见。

## 当前基线

当前 App-owned 子系统生命周期、特有 render 能力显式调用和 app-kit 状态 owner 见
[`../summaries/runtime-app-subsystem-boundaries.md`](../summaries/runtime-app-subsystem-boundaries.md) 和
[`../summaries/frame-lifecycle.md`](../summaries/frame-lifecycle.md)。

## 待推进内容

- 梳理 App 内部静态装配点：明确新增、拆卸 GUI / pipeline / debug 能力时需要修改的字段、init、resize、shutdown 和 pass 调用。
- 评估 pipeline 能力边界：按具体 owner 收敛设备能力检测、窗口尺寸资源、graph 贡献、debug image 输出与释放职责，
  不预先引入统一 render trait。
- 梳理 builtin subsystem：将 GUI、camera/input、overlay、pipeline controls 等 app-kit 能力保持为职责清楚、
  可跨多个 App phase 调用的具体类型。
- 设计分层事件模型：区分 platform input/window 事件、app 业务事件和 world edit 意图；避免引入全局万能 event bus。
- 评估 app-kit 拆分：当 builtin feature 边界稳定后，再决定是否拆分 crate 或模块。

## 边界与非目标

- 不让 runtime 或 Runner 感知具体子系统类型、GUI 或 pipeline 策略。
- `SubsystemLifecycle` 只保留 init / resize / shutdown，不加入 input、update、render 或 pass 调度。
- 不引入运行时注册表、依赖拓扑排序、动态安装、统一 tick 分发或万能 event bus。
- 具体 App 按自身需要手写静态组合；纯 UI / CPU 对象不强制实现生命周期 trait。

## 完成标准

- 每个可选子系统的静态装配位置与拆卸影响可从具体 App 字段和 phase 调用点直接看出。
- pipeline 的初始化、resize、graph 贡献和 shutdown 顺序可由具体 App 代码直接看出。
- GUI/input/overlay 的消费顺序仍由 App 策略明确控制。
- app-kit 中可复用子系统的 owner、非职责和依赖方向能写入模块 README 或 summaries。
