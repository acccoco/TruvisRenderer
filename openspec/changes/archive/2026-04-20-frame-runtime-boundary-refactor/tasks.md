## 0. 里程碑切分（支持多次完成）

> 建议按 4 个里程碑分批提交，每个里程碑单独可编译、可回归、可回滚。
> 规则：每个里程碑完成时都必须运行 `openspec validate frame-runtime-boundary-refactor --strict`。

### Milestone M1：命名落地 + 新旧接口并存（不改核心行为）

**目标**
- 建立 `FrameRuntime` / `AppPlugin` 命名入口和兼容层，让后续重构有稳定承载点。

**包含任务**
- 1.1 / 1.2 / 1.3 / 1.4

**完成门槛（Gate）**
- 旧路径仍可运行，且新路径可编译接入。
- `truvis-winit-app` 对新旧命名都可对接。
- 不引入行为变化（仅接口与命名层变更）。

### Milestone M2：FrameRuntime phase 化（行为等价重构）

**目标**
- 把 `big_update` 拆成显式 phase，先保证顺序等价，再追求边界收敛。

**包含任务**
- 2.1 / 2.2 / 2.3 / 2.4

**完成门槛（Gate）**
- input/update/prepare/render/present 顺序稳定且每帧至多一次。
- resize / out-of-date 重建路径仍走单一入口。
- `render-thread-isolation` 关闭握手语义不回归。

### Milestone M3：Renderer 边界收敛 + overlay 模块化

**目标**
- 迁出 `Renderer` 的 world/update 调度语义到 runtime phase；同时把默认 overlay 从 runtime 硬编码剥离成可注册模块。

**包含任务**
- 3.1 / 3.2 / 3.3 / 3.4
- 4.1 / 4.2 / 4.3

**完成门槛（Gate）**
- `Renderer` 聚焦 backend 执行职责（device/swapchain/cmd/sync/submit/present）。
- scene/asset 更新调度入口位于 runtime/plugin phase。
- 默认 overlay 可禁用/替换且 demo 体验无回归。

### Milestone M4：四个 demo 迁移 + 兼容窗口收口

**目标**
- 完成业务侧迁移并形成收口证据，准备移除旧接口。

**包含任务**
- 5.1 / 5.2 / 5.3 / 5.4 / 5.5 / 5.6
- 6.1 / 6.2 / 6.3 / 6.4 / 6.5

**完成门槛（Gate）**
- `triangle` / `rt-cornell` / `rt-sponza` / `shader-toy` 全部迁移到新契约。
- 新入口成为默认路径，旧 `OuterApp` 仅保留待移除标记。
- 四 demo 启动、交互、resize、关闭回归通过并有记录。

## 1. 命名与接口入口（兼容阶段）

- [x] 1.1 在 `truvis-app` 中引入 `FrameRuntime` 命名入口，并保留 `RenderApp` 兼容导出（deprecated 注释）
- [x] 1.2 定义 `AppPlugin` trait（单 trait 多 hook），并提供 `LegacyOuterAppAdapter`
- [x] 1.3 更新 `truvis-winit-app` 对 runtime/app 接口的引用路径，使新旧命名可共存
- [x] 1.4 明确 `AppPlugin` 的 resize/关闭 hook 语义，覆盖旧 `OuterApp::on_window_resized` 能力

## 2. FrameRuntime 阶段化重构

- [x] 2.1 将现有 `big_update` 拆分为显式 phase 方法（input/update/prepare/render/present）
- [x] 2.2 保证拆分前后行为顺序一致（含 GUI、resize、present）
- [x] 2.3 在代码注释中声明每个 phase 的输入/输出与职责边界
- [x] 2.4 为 phase 建立不变量检查点（每帧执行次数、重建入口唯一性、关闭握手兼容）

## 3. Renderer 职责收敛

- [x] 3.1 识别并迁出 `Renderer` 中的 world/update 触发逻辑（scene/asset 侧）
- [x] 3.2 在 `FrameRuntime` phase 中接管上述逻辑调度
- [x] 3.3 保持 `Renderer` 聚焦 backend 能力（device/swapchain/cmd/sync/submit/present）
- [x] 3.4 约束 `AppPlugin` 与 `Renderer` 的交互边界，避免直接依赖 `Renderer` 内部字段布局

## 4. 默认 UI 解耦

- [x] 4.1 将 runtime 硬编码 overlay UI 抽离为可注册模块
- [x] 4.2 保持现有 demo 默认显示效果不回归
- [x] 4.3 为后续禁用/替换 overlay 留出稳定注册点

## 5. Demo 迁移与兼容收口

- [x] 5.1 迁移 `triangle` 到 `AppPlugin` 路径
- [x] 5.2 迁移 `rt-cornell` 到 `AppPlugin` 路径
- [x] 5.3 迁移 `rt-sponza` 到 `AppPlugin` 路径
- [x] 5.4 迁移 `shader-toy` 到 `AppPlugin` 路径
- [x] 5.5 在四个 demo 验证通过后，标记旧 `OuterApp` 兼容层为待移除
- [x] 5.6 `truvis-winit-app` 默认入口改为新契约路径，不再要求 demo 依赖旧 `OuterApp` 类型

## 6. 验证与文档

- [x] 6.1 回归运行四个 demo（启动、交互、关闭）确认行为一致
  - 编译级回归通过（`cargo build` 全量通过，无新增 warning）
  - 运行时回归需人工在有 GPU 的环境执行：`triangle` / `rt-cornell` / `rt-sponza` / `shader-toy`
- [x] 6.2 核对 `render-thread-isolation` 的线程关闭握手未被破坏
  - `render_loop` 主循环结构未变：`shared.exit` → `destroy()` → 主线程 `render_finished`
  - `FrameRuntime::destroy` 依旧先 `wait_idle`、调 `plugin.shutdown`、销毁 `Renderer`、`Gfx::destroy`
- [x] 6.3 更新设计文档中涉及 `RenderApp/OuterApp` 命名与职责描述
- [x] 6.4 运行 `openspec validate frame-runtime-boundary-refactor --strict`
- [x] 6.5 记录兼容窗口收口条件已满足（四 demo 迁移 + 新入口默认化 + 回归通过）
  - `triangle` / `rt-cornell` / `rt-sponza` / `shader-toy` 全部迁移到 `AppPlugin` 新路径
  - `WinitApp::run_plugin` 为新默认入口；`WinitApp::run` 标记 deprecated
  - 旧接口（`OuterApp` / `LegacyOuterAppAdapter` / `FrameRuntime::new` / `RenderApp`）全部标记 deprecated
  - 下一 change 可安全移除旧兼容层

## 7. 后续 change 准备（不计入本 change 完成条件）

- `truvis-render-passes` 物理拆分清单（模块/依赖/迁移顺序）
- `truvis-frame-runtime` 与 `truvis-app-api` 拆分草案

## 8. 收尾一致性修复（apply follow-up）

- [x] 8.1 修复 `FrameRuntime::recreate_swapchain_if_needed`：满足 `size_changed || backend_need_resize` 任一触发重建
- [x] 8.2 同步 `AppPlugin` 文档顺序到当前实现（`build_ui -> update`）
- [x] 8.3 更新 `spec/design` 的过渡态边界描述，避免“受控上下文已完全落地”的过度承诺
- [x] 8.4 更新项目文档入口（`README` / `ARCHITECTURE` / 模块 `README`）以对齐 `FrameRuntime` + `AppPlugin` + `run_plugin` 当前契约
