## Why

RenderGraph 当前暴露了拓扑排序、transient 资源、buffer 访问和 compiled graph 等通用引擎式抽象，但项目实际使用的是 App 在 render hook 中显式添加 pass 并决定顺序。这个差异让模块理解成本偏高，也让未完全落地的 API 给调用方造成错误预期。

## What Changes

- **BREAKING**: RenderGraph 的执行顺序固定为 pass 添加顺序，不再通过依赖图或拓扑排序重排 pass。
- 将资源读写声明的职责收敛为 barrier 计算和校验，不承担 pass 调度。
- 移除或私有化当前未完整实现的 public API，包括 transient image/buffer 创建、buffer barrier 录制入口和依赖图调试类型。
- 收敛 per-frame graph 构建与执行路径，保留当前 demo 真实需要的 imported image、image state、image barrier、external semaphore 和 pass record 能力。
- 更新 RenderGraph 文档，明确它是按帧命令录制与同步辅助模块，不是自动调度系统。

## Capabilities

### New Capabilities

- `render-graph-sequential-execution`: 约束 RenderGraph 的顺序执行模型、资源访问声明、barrier 推导边界和 public API 范围。

### Modified Capabilities

None.

## Impact

- Affected crates: `truvis-render-graph`, `truvis-render-passes`, `truvis-app`.
- Affected APIs: RenderGraph pass registration/compile/execute surface may be simplified; unsupported public re-exports may be removed.
- Affected docs: `ARCHITECTURE.md`, `engine/crates/truvis-render-graph/README.md`, and any module README mentioning topology sorting or graph transient resources.
- Expected behavior: Render output and frame ordering should remain equivalent for existing demos; only internal ordering policy and unsupported API surface change intentionally.
