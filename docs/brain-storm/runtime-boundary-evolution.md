# Runtime 边界演进

## 目标

让 runtime phase、main view、surface 和 `Gfx` owner 边界更显式。目标不是改变现有渲染结果，
而是减少 prepare 入口承载过多语义、view 状态分散和 surface 生命周期难以扩展的问题。

## 当前基线

当前 runtime / Renderer / subsystem phase、状态 owner 和资源生命周期见
[`../summaries/frame-lifecycle.md`](../summaries/frame-lifecycle.md)、
[`../summaries/runtime-renderer-subsystem-boundaries.md`](../summaries/runtime-renderer-subsystem-boundaries.md) 和
[`../summaries/threading-and-resource-lifecycle.md`](../summaries/threading-and-resource-lifecycle.md)。
本文件只记录这些边界之后的演进方向。

## 待推进内容

- 拆分 prepare 语义：把 CPU scene snapshot、render-side manager resolve、GPU upload、per-view data 和 descriptor update
  的职责边界显式化，但保持 update/render 阶段的访问约束不变。
- 引入 prepared main view：让 runtime 在 prepare 边界形成只读的 view 快照，逐步把 per-frame 中 camera/projection/extent
  相关字段收敛为 per-view 语义。
- 评估多 view / 多 surface / headless：先定义 surface 与 present owner 的最小抽象，再评估 editor viewport、
  offscreen rendering 或无窗口运行是否需要进入主线。
- 继续收敛 `Gfx` 访问：减少历史式全局访问和宽 owner 暴露，让长期对象继续只保存自身资源 handle，
  通过 phase-appropriate typed ctx 完成创建和销毁。

## 边界与非目标

- 不把 Renderer camera controller、GUI、overlay 或具体 pipeline 状态移入 runtime。
- 不在第一轮引入重型 view family、shadow atlas 或多 surface 调度器。
- 不改变所有 Vulkan 对象在渲染线程创建、使用和销毁的线程边界。
- 不把当前 runtime phase 事实重复写入本文件；事实变更应同步到 summaries。

## 完成标准

- prepare 内部职责可以从命名和调用关系上区分，并能说明每个子阶段读写的 owner。
- render hook 能读取 runtime 准备好的 main view 语义，而不是在 app 层重新拼装 frame/view 含义。
- surface/headless 方向有清晰的 owner、生命周期和非目标说明。
- 相关事实沉淀到 summaries 后，本文件只保留下一批尚未推进的 runtime 边界问题。
