# truvis-render-graph

按帧命令录制与同步辅助模块，负责按 App 添加 pass 的顺序推导 image 状态转换，并构建可执行计划。

## 提供能力

- `RenderGraphBuilder` 按 pass 添加顺序记录执行序列，不做拓扑排序或自动调度
- 导入已有 GPU image，并声明 pass 对这些 image 的读写状态
- 自动生成必要的 image barrier、layout transition 和 epilogue barrier
- 汇总 imported image wait semaphore、exported image signal semaphore 以及额外 signal semaphore
- 编译为 `CompiledGraph`，再执行 command recording 并生成 queue submit 所需信息
- 输出 execution plan，辅助定位 pass 顺序、资源访问、barrier 和 semaphore

## 顺序执行模型

- App 在 `RenderAppHooks::render` 中创建 per-frame graph，并按业务语义调用具体 Plugin 的 `contribute_passes`。
- pass 的添加顺序就是 command recording 顺序，也是最终执行顺序。
- `read_image` / `write_image` / `read_write_image` 声明只用于 barrier 推导、资源校验和调试输出，不参与 pass 调度。
- image 初始状态来自 `import_image`；后续线性扫描会处理 write-after-read、read-after-write、write-after-write、layout transition 和连续只读访问。

## 调试输出

`CompiledGraph::print_execution_plan()` 用于打印编译后的执行计划。输出会先组装为一段多行文本，再通过一次 `info` 日志写出，内容包含 pass 顺序、image 读写、pass 前 barrier、epilogue barrier 和 semaphore 数量，便于从日志中整体复制和分析。

## 边界约束

- 仅关注 imported image 的状态跟踪、同步和命令录制辅助，不依赖 scene/asset 等领域模块
- App 和具体 Plugin 显式决定 pass 添加顺序，RenderGraph 不重排 pass
- transient image/buffer、buffer barrier 录制、多队列调度和资源 aliasing 暂不属于当前能力
- 业务 pass 逻辑在上层模块组织（如 `truvis-app`）
