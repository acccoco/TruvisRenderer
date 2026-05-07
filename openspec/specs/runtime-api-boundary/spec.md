## Purpose

定义 runtime/plugin/passes 的能力边界与迁移收口条件，约束 `FrameApp`、`FrameAppHooks`、`Plugin` typed contexts、`BaseApp` 单入口帧骨架、crate 物理拆分以及兼容层下线标准，确保运行时语义可验证且不回退既有分层规范。

## Requirements

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

### Requirement: App SHALL 通过 Plugin 和 FrameAppHooks 访问运行时能力

App（通过 `FrameAppHooks`）SHALL 在各 hook 中接收 RenderBackend 的 typed Ctx，并自主裁剪后传给 Plugin。Plugin SHALL 通过 `PluginInitCtx` / `PluginUpdateCtx` / `PluginRenderCtx` / `PluginResizeCtx` 访问能力。

#### Scenario: App hook 接收 RenderBackend Ctx

- **WHEN** `FrameAppHooks::update` 被 BaseApp 调用
- **THEN** App 接收 `&mut RenderBackendUpdateCtx`，可从中构造 `PluginUpdateCtx` 传给 Plugin

#### Scenario: Plugin 不直接接触 RenderBackend Ctx

- **WHEN** Plugin 需要读取帧状态或提交渲染命令
- **THEN** SHALL 通过 Plugin 层 Ctx（`PluginInitCtx` / `PluginUpdateCtx` / `PluginRenderCtx`）完成
- **AND** Plugin 代码 SHALL NOT 依赖 `RenderBackendUpdateCtx` 或 `RenderBackendRenderCtx` 的具体字段布局

### Requirement: BaseApp SHALL 作为帧编排单入口

`BaseApp` SHALL 作为渲染线程中的帧骨架入口，render_loop 通过 `Box<dyn FrameApp>` 驱动帧推进。`FrameApp::run_frame` 内部通过 `BaseApp::run_frame` 执行骨架。

#### Scenario: render loop 通过 FrameApp 驱动

- **WHEN** 渲染线程主循环推进单帧
- **THEN** 调度逻辑 SHALL 通过 `FrameApp::run_frame()` 完成
- **AND** render_loop SHALL NOT 直接访问 RenderBackend 或 BaseApp

#### Scenario: winit 入口表达 App 而非 Plugin

- **WHEN** 外部启动 demo app
- **THEN** SHALL 使用 `WinitApp::run_app(|| Box<dyn FrameApp>)` 或等价 App factory 入口
- **AND** 新入口命名 SHALL NOT 暗示传入的是单一 `FramePlugin`

#### Scenario: 帧节流决策点唯一

- **WHEN** 系统判定是否推进下一帧
- **THEN** render_loop SHALL 通过 `FrameApp::time_to_render()` 查询
- **AND** App SHALL 委托给 `BaseApp::time_to_render()`（内部调 `RenderBackend::time_to_render()`）
- **AND** 节流判断 SHALL 仅在此单一链路执行

### Requirement: prepare 阶段职责 SHALL 由 BaseApp 帧骨架持有

prepare 阶段的调度与执行 SHALL 由 `BaseApp::run_frame` 的帧骨架固定执行。Plugin 不引入独立 prepare hook。

#### Scenario: Plugin 契约不包含 prepare hook

- **WHEN** 定义 `Plugin` 生命周期 hook
- **THEN** 契约 SHALL 包含 `init / on_input / update / on_resize / shutdown`
- **AND** SHALL NOT 暴露独立 `prepare` hook

#### Scenario: prepare 在 render 前由 BaseApp 骨架完成

- **WHEN** BaseApp 执行帧骨架
- **THEN** SHALL 在 `app.render()` 前调用 `render_backend.prepare(app.camera())`

### Requirement: Demo SHALL 迁移到 FrameApp + Plugin 架构

四个官方 demo SHALL 迁移为实现 `FrameApp` + `FrameAppHooks` 的 App struct，内部持有具体 Plugin。

#### Scenario: 四 demo 完成迁移

- **WHEN** 运行 `triangle` / `rt-cornell` / `rt-sponza` / `shader-toy`
- **THEN** 四者均 SHALL 实现 `FrameApp` + `FrameAppHooks` trait
- **AND** 各 app 内部 SHALL 持有 `GuiPlugin` 和对应的渲染 Plugin

### Requirement: crate 边界 SHALL 反映新架构

- `truvis-frame-api`：SHALL 定义 `Plugin` trait、`FrameApp` trait、`FrameAppHooks` trait 和所有 Plugin Ctx 类型
- `truvis-frame-runtime`：SHALL 定义 `BaseApp` struct 和帧骨架实现

#### Scenario: Plugin trait 和 App trait 在 frame-api 中

- **WHEN** 外部 crate 需要实现 Plugin 或 FrameApp
- **THEN** SHALL 从 `truvis-frame-api` 导入相关 trait 和 Ctx 类型

#### Scenario: BaseApp 在 frame-runtime 中

- **WHEN** App 需要使用帧骨架
- **THEN** SHALL 从 `truvis-frame-runtime` 导入 `BaseApp`
