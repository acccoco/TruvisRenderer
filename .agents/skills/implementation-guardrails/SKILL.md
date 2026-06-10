---
name: implementation-guardrails
description: 修改代码时使用的实现约束。适用于实现功能、修复 bug、重构、调整 Rust/C++/shader/RenderGraph/FFI 代码，或新增模块、函数、资源流程和公共接口时，要求先复用已有实现、避免过度抽象、保持函数归属于明确的 struct/impl 或模块边界，并为关键改动补充详细中文注释。
---

# 实现约束

## 工作流程

1. 修改前先读取 `AGENTS.md`、`docs/ARCHITECTURE.md`、相关 `docs/summaries/`、模块 `README.md` 和相邻代码，确认当前职责边界与已有模式。
2. 新增逻辑前搜索已有工具函数、类型、trait、模块、crate、shader helper 或平台封装；能复用或扩展现有边界时，优先复用，不重复造轮子。
3. 只在真实需要表达状态、资源、生命周期、复用压力或边界契约时新增抽象；避免 Java 式层层封装、空壳 manager、过早泛化和为了单个分支创建通用层。
4. 新增函数默认归属到承担对应状态、资源、流程或不变量的 `struct/impl` 中；不要新增无归属的零散 free helper。
5. 修改后同步维护相关活跃文档；除非用户明确要求，不主动新增单元测试。

## 结构约束

- 先找现有归属点：已有 `struct`、资源对象、pass、runtime、builder、adapter、trait impl 或明确职责的 helper 模块。
- 多个函数反复传递同一组 context、config、device、allocator、queue、buffer、descriptor 或 FFI 句柄时，应收敛到持有这些依赖的类型中。
- 必须按顺序调用、共享不变量、管理资源释放或跨帧状态的逻辑，应由一个类型表达生命周期和调用契约。
- 入口函数、trait impl、测试函数、极小局部闭包、语言或框架要求的 free function 可以保留，不为了形式强行包进无意义类型。
- 局部 helper 只有在高度特化、一次性、不会演变成共享职责时才接受；否则应迁移到已有公共边界或新增有明确所有权的类型。

## 注释与文档

- 项目自有解释性注释使用中文；API 名称、代码标识符、shader/FFI/RenderGraph 等领域术语可以保留英文。
- 注释解释设计意图、调用契约、边界不变量和全局上下文，不复述显而易见的代码行为。
- 涉及所有权、生命周期、同步、线程安全、资源释放、FFI ABI、GPU resource、descriptor、RenderGraph 依赖、shader payload 或坐标系假设时，必须说明关键不变量。
- 模块级文档应说明职责边界、非职责、主要抽象、对外契约、重要不变量和依赖关系。

## 提交前自检

- 是否搜索并复用了已有工具函数、类型或模块边界。
- 是否避免了为了单次需求新增过度抽象。
- 新增函数是否有清晰归属，并位于对应 `struct/impl` 或明确职责模块内。
- 关键生命周期、资源、同步、FFI、RenderGraph 和 shader 契约是否有中文解释性注释。
- 文档是否随代码事实同步更新。
