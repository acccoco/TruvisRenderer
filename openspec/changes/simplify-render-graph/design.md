## Context

当前 `truvis-render-graph` 同时包含 pass trait、lambda wrapper、resource manager、dependency graph、compiled graph、transient resource 描述、buffer 访问声明和 barrier 计算。实际调用方则集中在 App 的 `RenderAppHooks::render` 中：App 创建 graph，按期望顺序调用具体 Plugin 的 `contribute_passes`，最后录制命令并提交。

这个使用方式说明 RenderGraph 当前并不需要自动调度 pass。App 已经拥有业务顺序，RenderGraph 的核心价值应是根据 pass 声明的资源访问，在线性执行序列中插入必要的 Vulkan image barrier，并汇总外部 semaphore。

## Goals / Non-Goals

**Goals:**

- 将 pass 添加顺序定义为唯一执行顺序。
- 用线性资源状态跟踪替代 `DependencyGraph` / `petgraph` 拓扑排序。
- 修正资源状态跟踪边界，使读后写、写后读、写后写和 layout transition 都能通过 barrier 表达。
- 收窄 public API，仅暴露当前真实支持的 RenderGraph 能力。
- 保持现有 demo 的渲染输出、frame order、queue submit 行为等价。
- 同步更新文档，避免继续描述“拓扑排序执行”和尚未落地的 transient graph resource。

**Non-Goals:**

- 不实现自动 pass scheduling、pass culling、资源 lifetime aliasing 或多队列调度。
- 不实现 RenderGraph transient image/buffer 分配。
- 不补齐 buffer barrier 录制；buffer graph 能力在本 change 中应隐藏或移出 public contract。
- 不改变具体渲染 pass 的 shader、pipeline、descriptor 或 draw/dispatch 逻辑。

## Decisions

### D1: 添加顺序就是执行顺序

RenderGraph 不再根据资源依赖重排 pass。`add_pass` 的顺序就是 command recording 顺序，也是 App 和 Plugin 组合层看到的顺序。

理由：

- 项目架构已经要求 App 显式决定 GUI pass 与渲染管线 pass 顺序。
- 当前 pass 数量少、拓扑简单，自动排序收益很低。
- 自动排序会让 App 层顺序语义变得不稳定，尤其是 GUI overlay、present resolve 等需要明确位置的 pass。

备选方案是保留 `DependencyGraph` 只做拓扑排序。拒绝原因是它和 App 显式编排重复，而且当前实现并不完整地建模读后写依赖。

### D2: 资源声明只驱动 barrier 和校验

Pass 的 `read_image` / `write_image` / `read_write_image` 声明不再参与调度，只用于：

- 计算 pass 前 barrier。
- 校验资源句柄是否已导入或由受支持路径创建。
- 生成调试输出。

依赖关系从“决定执行顺序”降级为“解释为什么此处需要同步”。

### D3: 线性状态跟踪替代依赖图

Graph 在执行顺序上做一次线性扫描，为每个 image 维护当前状态：

- 初始状态来自 `import_image`。
- 写入后状态更新为写入声明的 `RgImageState`。
- 只读访问会参与状态跟踪；连续读可以合并 stage/access，后续写入需要等待这些读完成。
- layout 改变、写后读、读后写、写后写都应在 pass 前生成 barrier。

这个模型比通用 DAG 简单，但覆盖当前单 command buffer、单 graphics queue 的使用方式。

### D4: 收窄 public API 到已落地能力

Public surface 应保留：

- graph 构建与执行入口。
- imported image 注册。
- image read/write/read-write 声明。
- `RgImageHandle` / `RgImageState`。
- pass 执行上下文中的 image/view 查询。
- wait/signal semaphore 汇总。
- execution plan 调试输出。

Public surface 应移除或私有化：

- `DependencyGraph` / `EdgeData`。
- graph transient image/buffer 创建和描述类型。
- buffer read/write/state/barrier 相关入口，直到 buffer barrier 录制完整实现。
- 内部 resource manager/resource node/barrier desc 类型，除非调用方确实需要直接构造。

### D5: 保留 pass adapter 的最低必要形态

现有调用方有两种 pass 贡献方式：闭包 pass 和具体 `RgPass` 适配器。实现时可以先保留这两种调用形态，避免一次性修改所有 pass。关键是不要把类型擦除 wrapper、node、resource manager 等内部结构继续作为 public API 暴露。

后续如果调用方更偏向闭包式 graph，可另开 change 合并 `RgPass` trait 与 lambda pass。

## Risks / Trade-offs

**[失去自动并行调度可能性]** -> 当前架构没有多队列或 pass 并行执行需求；后续需要时再以新的 capability 引入。

**[public API 缩减影响调用方]** -> 调用方集中在 workspace 内，迁移范围可通过编译器捕获。任务中应先清点实际使用的 re-export 再删除。

**[barrier 行为回归]** -> 为读后写、写后读、写后写、layout transition、连续只读访问增加单元测试，并运行现有 demo 编译检查。

**[文档与实现短期不一致]** -> 本 change 必须同步更新 `ARCHITECTURE.md` 和 `truvis-render-graph/README.md`，删除拓扑排序和 transient resource 作为当前能力的表述。

## Migration Plan

1. 在 RenderGraph 内部先引入线性 execution order 和 image state tracker，保持现有调用方可编译。
2. 移除拓扑排序参与执行的路径，并删除 `petgraph` 依赖。
3. 收窄 public re-export 和未实现 API，按编译错误迁移 workspace 调用方。
4. 更新文档与 OpenSpec。
5. 运行格式化、编译检查和至少一个可行 demo smoke test。

## Open Questions

- 是否保留 `RenderGraphBuilder::compile()` 名称作为过渡 API，还是在本 change 中直接改成 `RenderGraph::record/execute`。建议实现时优先选择调用方改动最小的方案，除非现有命名继续传递错误语义。
- `RgImageState` 是否应在后续 change 中移动到 `truvis-gfx` 或 `truvis-render-interface`。本 change 不处理这个迁移。
