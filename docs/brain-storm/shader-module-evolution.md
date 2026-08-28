# Slang Module 编译演进

## 目标

先测量当前四个 shader package 的全量与增量编译成本；只有 Engine 稳定 `lib/` 的重复前端编译成为明确瓶颈时，
再评估 Slang `module` / `.slang-module`，避免为了理论复用增加缓存与失效复杂度。

## 当前基线

当前源码 owner、package、include、depfile 和 binding 契约见
[`../summaries/layering-and-dependency-boundaries.md`](../summaries/layering-and-dependency-boundaries.md)、
[`../../engine/shader/README.md`](../../engine/shader/README.md) 与
[`../../renderer/shader/README.md`](../../renderer/shader/README.md)。现有源码以 canonical include 为主，
四个 package 的源码预检和 compiler depfile 校验已经落地。

最小可行性实验已证明：Engine `lib` 可以预编译为 `.slang-module`，Renderer entry 可以导入它；源码 module
与预编译 module 能生成相同 SPIR-V。该实验只证明机制可行，不证明生产构建已经值得承担 module cache。

## 待推进内容

- 记录 `just shader-force`、无改动 `just shader` 和 Engine shared input 变化时的编译耗时与重复前端成本。
- 若收益明确，为 module cache 定义 Slang 版本、编译参数、源码依赖和 package 配置共同参与的 freshness key。
- 保持源码预检与 compiler depfile 为依赖边界事实来源；module 不得绕过 owner 或 package 可见性。
- 先只评估稳定 Engine `lib`，不把 descriptor ABI、预处理宏依赖源码或 Renderer-local 算法机械模块化。

## 边界与非目标

- 不改变 Engine / Renderer owner 方向、四个 package、runtime artifact path 或 binding crate 所有权。
- 不与 shader 算法、ABI 布局、descriptor、pipeline 或 RenderGraph 修改混在同一阶段。
- 不引入运行时动态链接；最终 SPIR-V 仍是自包含程序。

## 完成标准

- 有可复现的编译耗时证据证明 module cache 的收益高于缓存与失效成本。
- 源码与预编译 module 路径生成的 SPIR-V 通过相同 validator，并保持关键 ABI offset、binding 和 push constant 契约。
- `just shader-force`、增量 `just shader`、binding build 与完整 workspace 构建均通过。
