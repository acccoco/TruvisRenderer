## ADDED Requirements

### Requirement: 帧编排类型语义明确为 FrameRuntime

主框架中的帧编排入口 SHALL 以 `FrameRuntime` 语义暴露，并承担阶段调度职责。旧 `RenderApp` 命名在过渡期可存在兼容导出，但 SHALL 明确标记为兼容路径。

#### Scenario: 新入口可用且旧入口兼容

- **WHEN** 调用方引入应用帧编排入口
- **THEN** SHALL 可以通过 `FrameRuntime` 命名访问
- **AND** 旧 `RenderApp` 路径在兼容窗口内仍可编译运行
- **AND** 旧路径 SHALL 被标记为 deprecated 或等价迁移提示

### Requirement: 应用扩展点升级为 AppPlugin（单 trait 多 hook）

应用侧扩展契约 SHALL 采用单 trait 的多阶段 hook 形式，以表达 update/ui/extract/render 等职责，避免单个 `OuterApp` 接口语义模糊。

#### Scenario: 新插件契约覆盖现有生命周期

- **WHEN** 应用实现 `AppPlugin`
- **THEN** SHALL 能表达初始化、更新、UI 构建、渲染相关 hook
- **AND** 框架 SHALL 按既定阶段顺序调用这些 hook

#### Scenario: 旧 OuterApp 路径可平滑迁移

- **WHEN** 现有 demo 仍基于旧 `OuterApp`
- **THEN** SHALL 可通过兼容 adapter 正常运行
- **AND** 迁移到 `AppPlugin` 后 SHALL 不改变线程模型与关闭语义

### Requirement: FrameRuntime SHALL 采用显式 phase 编排

`FrameRuntime` SHALL 以显式阶段函数组织每帧执行流程（至少包含 input/update/prepare/render/present），以替代单体更新函数的隐式边界。

#### Scenario: 阶段顺序稳定

- **WHEN** 运行任意一帧
- **THEN** input/update/prepare/render/present 阶段 SHALL 按固定顺序执行
- **AND** GUI、resize、present 的时序 SHALL 与现有行为保持等价

### Requirement: Renderer SHALL 收敛为 backend 职责

`Renderer` SHALL 聚焦 GPU backend 能力（device/swapchain/cmd/sync/submit/present），不得继续承载 scene/asset 侧 world 更新调度职责。

#### Scenario: world 更新由 runtime 驱动

- **WHEN** 发生 scene/asset 相关更新推进
- **THEN** 调度入口 SHALL 位于 `FrameRuntime` 的相应 phase
- **AND** `Renderer` SHALL 仅消费已准备好的渲染输入并执行 GPU 提交

### Requirement: 默认 overlay SHALL 可注册而非硬编码

默认调试/信息 overlay SHALL 通过可注册模块接入 runtime，而非固定写在核心编排路径中。

#### Scenario: 可替换默认 overlay

- **WHEN** 应用选择禁用或替换默认 overlay
- **THEN** SHALL 可以在不修改 `FrameRuntime` 核心流程的前提下完成
- **AND** 默认示例应用的用户体验 SHALL 保持不回归
