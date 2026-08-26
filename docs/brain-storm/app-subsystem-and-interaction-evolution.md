# Renderer-owned 子系统与交互边界演进

## 目标

在已经拆分的 App capability crate 基础上，继续收敛输入事件、业务交互与静态装配边界；保持资源 owner、
生命周期顺序和 RenderGraph 编排在具体 Renderer 中显式可见。

## 当前基线

当前 Renderer-owned 子系统生命周期、五个 capability crate 的依赖方向及特有 render 能力显式调用见
[`../summaries/runtime-renderer-subsystem-boundaries.md`](../summaries/runtime-renderer-subsystem-boundaries.md) 和
[`../summaries/layering-and-dependency-boundaries.md`](../summaries/layering-and-dependency-boundaries.md)。

## 待推进内容

- 继续核对 Renderer 内部静态装配点：新增或拆卸 ImGui / rendering / debug 能力时，字段、init、resize、shutdown 和 pass 调用保持清晰。
- 在确有新增能力时评估设备能力检测、debug image 输出和资源释放是否仍归属于对应 subsystem，不预先引入统一 render trait。
- 设计分层事件模型：区分 platform input/window 事件、app 业务事件和 world edit 意图；避免引入全局万能 event bus。
- 评估相机、点选、desktop command 和 Editor 意图之间的实际共享交互边界，不为了概念统一提前设计框架。

## 边界与非目标

- 不让 runtime 或 RenderLoop 感知具体子系统类型、ImGui 或渲染策略。
- `SubsystemLifecycle` 只保留 init / resize / shutdown，不加入 input、update、render 或 pass 调度。
- 不引入运行时注册表、依赖拓扑排序、动态安装、统一 tick 分发或万能 event bus。
- 具体 Renderer 按自身需要手写静态组合；纯 UI / CPU 对象不强制实现生命周期 trait。

## 完成标准

- 每个可选子系统的静态装配位置与拆卸影响可从具体 Renderer 字段和 phase 调用点直接看出。
- 渲染子系统的初始化、resize、graph 贡献和 shutdown 顺序可由具体 Renderer 代码直接看出。
- GUI/input/overlay 的消费顺序仍由 Renderer 策略明确控制。
- App capability crate 中可复用能力的 owner、非职责和依赖方向能写入模块 README 或 summaries。
