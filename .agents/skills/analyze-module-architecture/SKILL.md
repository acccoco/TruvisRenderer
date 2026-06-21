---
name: analyze-module-architecture
description: Analyze a code module, package, directory, subsystem, service, page, or task from an architecture perspective. Use when Codex needs to explain or document module responsibilities, boundaries, public interfaces, internal structure, runtime flows, data flow, dependencies, lifecycle/state, concurrency, performance, or produce Mermaid class/component/dependency/sequence/data-flow/state diagrams grounded in source code and project docs.
---

# Analyze Module Architecture

## Core Workflow

1. Scope the target module first. If the module path or boundary is ambiguous, infer the smallest reasonable scope from the user request and state the assumption.
2. Read project guidance before analyzing code. In this repository, read `AGENTS.md`, `docs/ARCHITECTURE.md`, relevant `docs/summaries/`, and the module README first; then inspect manifests/build files, public entry points, tests, examples, and callers.
3. 从外到内映射模块：职责与边界、对外 API、入口调用方、外部依赖、集成点、已有本地模式、隐藏耦合，然后分析内部组件和运行流程。
4. Separate facts from inference. Cite concrete files and symbols for observed behavior; mark uncertain items as "推断" or "未确认".
5. Use diagrams only where they clarify structure or behavior. Prefer 2-4 diagrams for a normal report; add more only when the user asks for exhaustive documentation.
6. 以风险、未知项和后续核查收尾：指出所有权不清、边界歧义、未文档化不变量、隐藏顺序依赖、职责重复，以及需要继续源码核查的位置。

## 分析姿态

- 结论必须基于源码和当前文档，不把命名猜测当作事实。
- 当模块边界不清时，列出多个可能边界并说明取舍，不强行给唯一答案。
- 主动暴露隐藏复杂度：集成点、隐式顺序、共享状态、生命周期耦合和跨模块假设。
- 不实现代码或编辑文档，除非用户明确要求。

## Evidence Rules

- Prefer source code, tests, examples, and local documentation over naming guesses.
- Use search to identify public symbols, callers, event/message names, commands, background tasks, and state transitions.
- Treat exported APIs, documented entry points, CLI commands, routes, events, messages, and FFI surfaces as public unless code/docs show otherwise.
- Treat private files, unexported helpers, local structs/classes/functions, and implementation-only tests as internal unless they are intentionally consumed across module boundaries.
- For every major claim, include at least one file reference or explicitly label it as inference.

## Analysis Dimensions

For a comprehensive request, read `references/architecture-analysis-guide.md` and cover all sections. For a quick request, prioritize:

- 模块定位: responsibility, boundary, users, key concepts.
- 对外接口: public APIs, inputs/outputs/errors, stability, examples.
- 内部结构: components, key objects/classes/types, relationships, patterns.
- 运行流程: initialization, typical request/call path, async/background work, errors.
- 数据视角: models, sources/sinks, reads/writes, cache/persistence/queues.
- 依赖关系: internal/external dependencies, reverse dependencies, failure behavior.
- 生命周期与状态: startup/shutdown, object lifetime, states, resources.
- 并发与性能: thread safety, async model, locks/queues, bottlenecks, resource use.

## 图表选择

图表只在能澄清模块理解时使用，不作为装饰。

- 组件图（Component diagram）：展示模块边界、内部组成和职责拆分。
- 依赖图（Dependency graph）：展示入站 / 出站依赖和依赖方向。
- 时序图（Sequence diagram）：展示初始化、帧流程、请求路径或资源更新路径。
- 数据流图（Data-flow graph）：展示数据从哪里来、如何转换、在哪里被消费。
- 状态图（State diagram）：展示生命周期、资源所有权、重建 / 销毁状态或同步状态。
- 对比表（Comparison table）：当存在多个可能模块边界或重构方向时使用。

持久文档优先使用 Mermaid；ASCII 草图只接受用于快速探索对话。每个图必须有源码引用支撑，或明确标注为推断。

## Output Shape

Start with a short conclusion that names the module's role and main architectural boundary. Then use sections that match the user's requested depth:

1. 模块定位
2. 对外接口
3. 内部结构
4. 运行流程
5. 数据视角
6. 依赖关系
7. 生命周期与状态
8. 并发与性能
9. 风险、未知项与建议

Use file links and symbol names throughout. Include Mermaid diagrams near the section they explain, not all at the end.
