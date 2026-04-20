# Plugin / Pass / 事件总线演化建议

## 1. 背景

当前工程已经完成了关键边界收敛：

- 应用扩展统一走 `AppPlugin` + typed contexts
- 帧驱动统一走 `FrameRuntime` phase
- 通用 pass 已拆分到 `truvis-render-passes`
- GUI 适配层 (`GuiRgPass`) 与纯 Vulkan 录制层 (`GuiPass`) 已分层

这为下一步从“可运行”走向“可持续扩展”提供了稳定基础。


## 2. 当前状态总结

### 2.1 Plugin 体系

已具备“单插件入口 + 多 hook”模式：

- 生命周期：`init / build_ui / update / render / on_resize / shutdown`
- 运行方式：主线程建窗，渲染线程实例化并执行 plugin
- 优点：边界明确、迁移成本低、可控性强

当前主要不足：

- 入口仍是“单插件实例”，缺少插件容器与插件依赖声明机制
- 各能力模块更多是“代码组织”，还未全面升级为“声明式装配单元”


### 2.2 Pass 集成体系

已具备“RenderGraph 协议 + 上层组图”模式：

- `RgPass::setup/execute` 负责依赖声明与执行逻辑
- 应用层按场景组图：`import/export/add_pass/compile/execute`
- 通用 pass 在 `truvis-render-passes`，集成编排在 `truvis-app`

当前主要不足：

- 部分 pass 适配层仍直接持有完整 `RenderContext`
- 上下文权限面偏大，后续演进到更细粒度上下文时会增加维护成本


### 2.3 事件机制

目前已有两类机制：

- 主线程 -> 渲染线程：`crossbeam-channel` 输入通道
- resize：`AtomicU64` 最新尺寸共享

这套机制在线程边界上是有效的，但它还不是“通用事件总线”。


## 3. 演化目标

### 3.1 Plugin 目标

从“单插件 hook 回调”演进到“插件组合 + 依赖声明 + 生命周期装配”：

- 支持 `PluginGroup`（一组相关插件打包）
- 支持插件依赖关系（before/after/requires）
- 支持能力按层拆分：Platform/Main/Render/Tooling


### 3.2 Pass 目标

从“可运行组图”演进到“稳定分层 + 最小权限上下文”：

- pass `execute` 仅拿到最小所需输入
- pipeline 选择策略与具体 pass 组装解耦
- 渐进引入 `Extract/Prepare/Queue/Render` 语义


### 3.3 事件目标

从“输入通道”演进到“分层事件总线”：

- `PlatformEventBus`：窗口、输入、焦点、surface 状态
- `WorldEventBus`：逻辑层短生命周期事件
- 事件按边界分域，避免全局混用造成隐式依赖


## 4. 推荐演化路线

### P0：稳态收敛（短期）

目标：在不改变外部行为前提下补齐可维护性。

- 明确并文档化 plugin hook 时序与职责
- 梳理 pass 中对完整 `RenderContext` 的依赖，列出最小化清单
- 继续保持 GUI 分层边界不回退

产出：

- 依赖矩阵（plugin / pass / runtime）
- 上下文最小权限迁移清单


### P1：插件容器化（中期）

目标：从“单插件入口”升级为“插件组合装配”。

- 引入 `PluginRegistry` / `PluginGroup`
- 支持插件声明依赖关系
- 将输入桥接、UI Overlay、管线策略等能力独立插件化

建议保留兼容路径：

- 旧入口可自动包裹成单元素 `PluginGroup`


### P2：Pass 特性化 + 策略层（中期）

目标：支持多渲染管线并存、选择和回退。

- 定义统一 `PipelineFeature` 契约（能力检测、阶段钩子、图注册）
- 拆分 `Raster/RT/ShaderToy` 为并列特性插件
- 引入 `PipelineManager` 统一切换与 fallback


### P3：分层事件总线（中长期）

目标：建立可追踪、可测试的事件流。

- 先引入 `PlatformEventBus`，承接窗口与输入语义事件
- 再引入 `WorldEventBus`，替代跨模块回调
- 采用双缓冲事件队列，保证帧边界语义稳定

约束：

- 禁止“全局任意发布/订阅”
- 事件类型必须归属明确域，避免跨层污染


## 5. 设计原则（执行期间保持不变）

1. Runtime 仍是帧编排唯一入口，不回退到多入口驱动
2. Renderer 聚焦 backend 职责，不回灌应用语义
3. RenderGraph 只做图编排，不承载领域对象
4. GUI 继续遵循“适配层在上层，录制层在 backend”
5. 新机制优先“兼容迁移”，避免一次性破坏


## 6. 风险与规避

### 风险 A：插件化后顺序变复杂

- 规避：强制依赖声明 + 拓扑校验 + 启动期报错

### 风险 B：事件总线导致隐式依赖

- 规避：按域分总线；要求事件 schema 与消费方可追踪；禁止全局总线泛化

### 风险 C：上下文最小化影响开发效率

- 规避：先提供兼容视图，再分批迁移 pass，不一次性重写


## 7. 可验收结果

达到以下条件可视为演化有效：

- 新增一条渲染特性能以插件方式接入，无需修改主循环
- 新增一类窗口/平台事件只改 PlatformEventBus 与订阅系统
- 至少一组 pass 完成最小上下文化，不再依赖完整 `RenderContext`
- 多管线切换可通过策略层完成，不出现主循环巨型分支


## 8. 非目标

本轮演化不追求：

- 一次性引入完整 ECS 框架替换现有结构
- 一次性完成全部 pass 上下文重写
- 为事件系统引入跨层全局万能总线

重点是“边界先稳、能力分层、渐进替换”。
