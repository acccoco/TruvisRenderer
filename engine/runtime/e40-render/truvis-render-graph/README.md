# truvis-render-graph

按帧命令录制与同步辅助模块，负责按 Renderer 添加 pass 的顺序推导 image 状态转换，并构建可执行计划。

## 提供能力

- `RenderGraphBuilder` 按 pass 添加顺序记录执行序列，不做拓扑排序或自动调度
- 导入已有 GPU image，并声明 pass 对这些 image 的读写状态
- 自动生成必要的 image barrier、layout transition 和 epilogue barrier
- 汇总 imported image wait semaphore、exported image signal semaphore 以及额外 signal semaphore
- 编译为 `CompiledGraph`，再执行 command recording 并生成 queue submit 所需信息
- 输出 execution plan，辅助定位 pass 顺序、资源访问、barrier 和 semaphore
- 执行阶段只通过 `GfxResourceAccess` 查询 imported image / image view，不依赖 runtime 的 concrete resource manager

## 顺序执行模型

- Renderer 在 `Renderer::render` 中创建 per-frame graph，并按业务语义调用具体渲染子系统的 `contribute_passes`。
- pass 的添加顺序就是 command recording 顺序，也是最终执行顺序。
- `read_image` / `write_image` / `read_write_image` 声明只用于 barrier 推导、资源校验和调试输出，不参与 pass 调度。
- image 初始状态来自 `import_image`；后续线性扫描会处理 write-after-read、read-after-write、write-after-write、layout transition 和连续只读访问。
- 同一 graph 只允许导入一次相同的物理 image handle；不同 view 不构成独立同步域。
  pass 和 export 必须引用本 graph 已登记的 handle；null image 和显式 null view 在导入时拒绝，
  不需要 view 时传 None。登记校验不证明 native 资源存活，也不能识别不同 graph 中碰巧相同的 SlotMap key。
- 同一 pass 多次声明同一 image 时，compile 合并全部 stage/access；layout 不同则拒绝编译。
  合并后的范围用于 pass 前依赖和后续 pass 的来源状态，不替代 pass 内部命令间的屏障。
- 连续读取扩展 stage/access 时继续建立依赖；仅当目标读取范围已被来源覆盖才省略同 layout
  的屏障。export 使用同一判定，相同的写状态仍生成依赖，不能用状态相等推断 GPU 已完成。
- execute 在任何 graph 命令录制前核验全部导入：image/view 存活、image 格式、view 来源和
  native image 唯一性。import 的 format 是 image 原始格式，view 可按合法规则重解释。
  无 view 的导入保留 None，查询 view 不返回 null 占位；资源映射须在一次 execute 内保持稳定。
- graph 的 barrier 覆盖全部 mip/layer，所有子资源须共享声明的 layout；不支持独立子资源状态。
- 一个 graph 对应一个提交 batch：同 handle 的 wait 合并 stage，timeline 下限取最大值；
  同值 signal 合并 stage，不同 signal 值或 binary/timeline 声明冲突被拒绝。
  timeline signal 必须大于本 batch 的 wait；native 类型、当前计数和设备限制仍由调用方保证。

## 调试输出

`CompiledGraph::print_execution_plan()` 用于打印编译后的执行计划。输出会先组装为一段多行文本，再通过一次 `info` 日志写出，内容包含 pass 顺序、image 读写、pass 前 barrier、epilogue barrier 和 semaphore 数量，便于从日志中整体复制和分析。

## 边界约束

- 仅关注 imported image 的状态跟踪、同步和命令录制辅助，不依赖 scene/asset 等领域模块
- 不依赖 `truvis-render-runtime`，也不持有 `GfxResourceRegistry`、descriptor 或 bindless owner
- Renderer 和具体渲染子系统显式决定 pass 添加顺序，RenderGraph 不重排 pass
- transient image/buffer、buffer barrier 录制、多队列调度和资源 aliasing 暂不属于当前能力
- 业务 pass 逻辑在 Renderer 层组织
- pass 名称在 setup 前检查 NUL；录制中的其它 panic 仍可能留下部分 command，必须丢弃失败结果。
- `build_submit_info` 只投影声明；调用方保证对应 command 已完整录制并 end，native 资源存活到 GPU 使用完成。
- 编译计划不回写长期 image 状态；重复使用前必须恢复 import 状态及 semaphore 前置。
