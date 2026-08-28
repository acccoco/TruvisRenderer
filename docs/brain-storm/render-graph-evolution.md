# RenderGraph 演进

## 目标

让 RenderGraph 覆盖更多本帧 GPU 工作的资源依赖、同步和调试信息，同时保持当前线性、可读、可逐步迁移的模型。
目标不是一次性变成完整 RDG，而是先补足当前 graph 难以校验和难以表达的资源边界。

## 当前基线

当前 RenderGraph pass 编排、image 状态声明、compute / present graph 组织和同步 raycast 边界见
[`../summaries/render-graph-and-data-flow.md`](../summaries/render-graph-and-data-flow.md)。

## 待推进内容

- 扩展 resource model：增加 buffer resource、buffer state 和 buffer barrier；后续再评估 acceleration structure
  read state 与 readback ticket。
- 增加跨 graph 状态校验：让 compute graph export 与 present graph import 的状态匹配能被工具或 builder 检查，
  降低人工协议风险。
- 简化 adapter 层：给 `RgPassContext` 增加更窄的 image/buffer resolve helper，减少每个 pass 重复查 handle、
  view、format 和 extent 的样板代码。
- 引入 FrameGraph 原型：在不改变线性执行的前提下，让 pipeline / 子系统贡献 scope 或 subgraph，并通过 typed output
  连接主视图、present 和 GUI。
- 评估异步 readback：把低频异步 picking 或 debug readback 表达为 graph output ticket；同步交互查询继续保留为 runtime 特例。

## 边界与非目标

- 不让底层 Gfx pipeline 直接依赖 graph handle。
- 不把 runtime prepare、asset upload 或 present owner 强行塞进 graph。
- 不在第一轮做 pass culling、transient aliasing、async queue scheduler 或完整可视化工具。
- 不改变 Renderer 显式决定 pass 顺序的策略，除非 FrameGraph 原型已经能保持同等可读性。

## 完成标准

- graph 能表达 image 之外的至少一类资源访问，并能生成对应 barrier 或明确拒绝未支持场景。
- 跨 graph import/export 状态不再完全依赖调用者手动对齐。
- 典型 RT / present / GUI path 的 adapter 样板减少，且底层 pass 仍可脱离 graph 复用。
- FrameGraph 原型能够描述主视图到 present 的 typed output 关系，且不引入新的 app/runtime 反向依赖。
