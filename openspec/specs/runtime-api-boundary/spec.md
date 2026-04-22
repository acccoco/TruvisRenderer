## Purpose

定义 runtime/plugin/passes 的能力边界与迁移收口条件，约束 `AppPlugin` typed contexts、`FrameRuntime` 单入口编排、crate 物理拆分以及兼容层下线标准，确保运行时语义可验证且不回退既有分层规范。

## Requirements

### Requirement: AppPlugin SHALL 通过 typed contexts 访问运行时能力

`AppPlugin` SHALL 使用按阶段划分的上下文类型访问运行时能力，而非直接接收 `Renderer` 全量对象。

#### Scenario: Hook 签名使用上下文类型

- **WHEN** 定义或实现 `AppPlugin` 的 `init/build_ui/update/render/on_resize` hook
- **THEN** 每个 hook SHALL 接收对应阶段的上下文对象（或等价的受控上下文）
- **AND** hook 签名中 SHALL NOT 暴露完整 `Renderer` 作为通用参数

#### Scenario: 上下文能力面受控

- **WHEN** `AppPlugin` 需要读取帧状态、提交渲染命令、访问 UI 数据或 resize 信息
- **THEN** SHALL 通过上下文公开的稳定接口完成
- **AND** 插件代码 SHALL NOT 依赖 `Renderer` 内部字段布局

### Requirement: FrameRuntime SHALL 成为帧编排单入口

`FrameRuntime` SHALL 作为渲染线程中的唯一帧编排入口，外部调度方不得绕过 runtime 直接操作其内部状态。

#### Scenario: render loop 仅通过 runtime API 驱动

- **WHEN** 渲染线程主循环推进单帧
- **THEN** 调度逻辑 SHALL 通过 runtime 的公开 API 完成（输入灌入、重建判定、单帧推进）
- **AND** render loop SHALL NOT 直接读写 runtime 内部字段

#### Scenario: 帧节流决策点唯一

- **WHEN** 系统判定是否推进下一帧
- **THEN** `time_to_render` 或等价节流判断 SHALL 仅在单一决策点执行
- **AND** 不得在 loop 与 runtime 内重复判定导致语义分叉

### Requirement: prepare 阶段职责 SHALL 由 FrameRuntime 唯一持有

prepare 阶段的调度与执行顺序 SHALL 由 `FrameRuntime` 唯一持有；`AppPlugin` 不引入独立 prepare hook。

#### Scenario: 插件契约不包含 prepare hook

- **WHEN** 定义 `AppPlugin` 生命周期 hook
- **THEN** 契约 SHALL 包含 `init/build_ui/update/render/on_resize/shutdown`（或语义等价集合）
- **AND** SHALL NOT 暴露独立 `prepare` hook 造成职责分叉

#### Scenario: prepare 在 render 前由 runtime 完成

- **WHEN** 推进单帧执行
- **THEN** `FrameRuntime` SHALL 在 `render` 前完成 prepare 阶段所需的 runtime/backend 工作
- **AND** plugin 渲染入口仅消费 prepare 后的稳定输入

### Requirement: Demo SHALL 迁移到上下文化插件契约

四个官方 demo SHALL 完整迁移到 typed contexts 插件契约。

#### Scenario: 四 demo 完成迁移

- **WHEN** 运行 `triangle` / `rt-cornell` / `rt-sponza` / `shader-toy`
- **THEN** 四者均 SHALL 通过新的上下文化 `AppPlugin` 路径接入 runtime
- **AND** 不再依赖旧 `OuterApp` 兼容路径

### Requirement: crate 边界 SHALL 映射为 runtime/api/passes 分层

工程 SHALL 完成 runtime/api/passes 的物理拆分，以匹配职责边界。

#### Scenario: app-api 与 frame-runtime 拆分

- **WHEN** 完成 M4 里程碑
- **THEN** 插件契约与上下文 SHALL 位于 `truvis-app-api`
- **AND** 帧编排实现 SHALL 位于 `truvis-frame-runtime`

#### Scenario: render-passes 拆分

- **WHEN** 完成 M5 里程碑
- **THEN** 通用 pass 实现 SHALL 位于 `truvis-render-passes`
- **AND** runtime crate SHALL NOT 继续承载通用 pass 逻辑

### Requirement: gui pass 分层 SHALL 与既有规范保持一致

`runtime-api-crate-split` 在进行 pass 拆分时 SHALL 继续满足 `gui-pass-separation` 的分层要求。

#### Scenario: GuiRgPass 归属保持应用集成层

- **WHEN** 执行 `render_pipeline` 迁移与 pass crate 拆分
- **THEN** `GuiRgPass` SHALL 保持在应用集成层（如 `truvis-app` 或其等价上层集成 crate）
- **AND** 不得把 `GuiRgPass` 下沉到 `truvis-gui-backend`

#### Scenario: gui-backend 与 render-graph 解耦不回退

- **WHEN** 完成本 change 的 crate 调整
- **THEN** `truvis-gui-backend` SHALL NOT 新增对 `truvis-render-graph` 的依赖
- **AND** `GuiPass` 继续作为纯 Vulkan 录制能力保留在 gui-backend

### Requirement: compatibility window SHALL 具备可执行收口

兼容窗口 SHALL 以可验证条件结束，避免旧接口长期滞留。

#### Scenario: 兼容层下线

- **WHEN** typed contexts 迁移、crate 拆分、四 demo 回归全部完成
- **THEN** `OuterApp` / `LegacyOuterAppAdapter` / `RenderApp` / `WinitApp::run` SHALL 被移除或彻底下线
- **AND** 文档与 OpenSpec SHALL 同步更新为最终结构

#### Scenario: truvis-app shim 全部下线后不残留 re-export 模块

- **WHEN** 兼容窗口关闭，所有 re-export shim 被移除
- **THEN** `truvis-app` 的 `lib.rs` SHALL NOT 包含仅由 `pub use other_crate::*` 构成的纯转发模块
- **AND** `truvis-app` 的 `Cargo.toml` SHALL NOT 保留仅因 re-export 而存在的依赖项（如 `truvis-logs`、`truvis-descriptor-layout-macro`、`ash-window`、`raw-window-handle`）
- **AND** `truvis-app/src/render_pipeline/` SHALL 仅保留属于应用集成层的 pass 编排（如 `rt_render_graph`），不包含已迁移到 `truvis-render-passes` 的通用 pass shim
- **AND** `truvis-app/src/platform/` 目录 SHALL 被完全移除，其职责已由 `truvis-frame-runtime` 承载

### Requirement: 文档与注释 SHALL 与边界改造同步维护

运行时边界、插件契约、迁移状态发生变化时，文档与关键注释 SHALL 在同一里程碑内同步更新。

#### Scenario: 边界变化后的文档同步

- **WHEN** 完成任意里程碑中的接口、流程或 crate 边界变更
- **THEN** `README` / `ARCHITECTURE` / 模块 README / OpenSpec 文档 SHALL 同步反映新边界
- **AND** 不得保留与现状冲突的旧叙述

#### Scenario: 关键注释同步

- **WHEN** phase 顺序、hook 语义、兼容窗口状态发生变化
- **THEN** 相关代码注释 SHALL 同步更新
- **AND** 过期迁移注释 SHALL 在兼容窗口结束时清理

### Requirement: 文件与模块命名 SHALL 与最终语义对齐

当 runtime/plugin/passes 的职责边界稳定后，文件名与模块名 SHALL 对齐最终语义；若存在迁移窗口，需提供兼容导入路径。

#### Scenario: 语义重命名执行

- **WHEN** 某文件或模块名称与其稳定职责语义不一致
- **THEN** 实现 SHALL 在对应里程碑执行重命名并修正引用路径
- **AND** 在兼容期 SHALL 提供 re-export 或迁移说明，避免一次性破坏调用方

### Requirement: 迁移过程 SHALL 避免并行重复实现

模块迁移到新 crate 时 SHALL 避免长期并行维护两套等价实现。

#### Scenario: 迁移采用 move + compatibility shim

- **WHEN** 把模块从旧位置迁移到新 crate
- **THEN** 旧路径 SHALL 通过 re-export/shim 转发到新实现，或在同里程碑内移除
- **AND** SHALL NOT 复制一份独立逻辑并长期并行维护