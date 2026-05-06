# truvis-render-graph

声明式渲染图模块，负责 pass 依赖编排、资源状态推导与执行计划构建。

## 提供能力

- 构建 pass DAG
- 声明图像/缓冲读写关系
- 自动生成必要同步与状态转换
- 编译后按拓扑顺序执行

## 调试输出

`CompiledGraph::print_execution_plan()` 用于打印编译后的执行计划。输出会先组装为一段多行文本，再通过一次 `info` 日志写出，内容包含 pass 顺序、资源读写、pass 前 barrier、epilogue barrier 和 semaphore 数量，便于从日志中整体复制和分析。

## 边界约束

- 仅关注图编排，不依赖 scene/asset 等领域模块
- 业务 pass 逻辑在上层模块组织（如 `truvis-app`）
