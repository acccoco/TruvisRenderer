---
name: analyze-module-architecture
description: Analyze a code module, package, directory, subsystem, service, page, or task from an architecture perspective. Use when Codex needs to explain or document module responsibilities, boundaries, public interfaces, internal structure, runtime flows, data flow, dependencies, lifecycle/state, concurrency, performance, or produce Mermaid class/component/dependency/sequence/data-flow/state diagrams grounded in source code and project docs.
---

# Analyze Module Architecture

## Core Workflow

1. Scope the target module first. If the module path or boundary is ambiguous, infer the smallest reasonable scope from the user request and state the assumption.
2. Read the nearest project guidance before analyzing code: architecture docs, module README files, manifests/build files, public entry points, tests, examples, and callers.
3. Map the module from the outside in: responsibilities and boundaries, public API surface, inbound callers, outbound dependencies, then internal components and flows.
4. Separate facts from inference. Cite concrete files and symbols for observed behavior; mark uncertain items as "推断" or "未确认".
5. Use diagrams only where they clarify structure or behavior. Prefer 2-4 diagrams for a normal report; add more only when the user asks for exhaustive documentation.
6. End with gaps, risks, and follow-up checks instead of silently filling unknowns.

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

## Diagram Rules

Use Mermaid syntax in Markdown.

- Class diagram: explain static types, traits/interfaces, inheritance/implementation, or object responsibility.
- Component diagram: explain internal pieces and module boundaries.
- Dependency graph: explain inbound/outbound module relationships and dependency direction.
- Sequence diagram: explain one representative call, request, or task flow.
- Data-flow graph: explain where data comes from, where it goes, and where it is transformed or stored.
- State diagram: explain lifecycle, state machine, resource states, or error/retry transitions.

Keep diagrams readable: use meaningful node names, avoid dumping every helper, and split large diagrams by concern.

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
