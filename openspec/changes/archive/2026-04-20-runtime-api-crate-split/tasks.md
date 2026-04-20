## 0. 里程碑切分（支持多次完成）

> 建议按 5 个里程碑分批提交。每个里程碑要求：可编译、可回归、可回滚。
> 规则：每个里程碑完成时都必须运行 `openspec validate runtime-api-crate-split --strict`。

### Milestone M1：AppPlugin 上下文类型化（保留兼容）

**目标**
- 把 `AppPlugin` 的能力面从 `Renderer` 全量访问收敛为 typed contexts，保持行为等价。

**包含任务**
- 1.1 / 1.2 / 1.3 / 1.4 / 1.5

**完成门槛（Gate）**
- 新旧插件路径均可编译运行（旧路径保留 deprecated）。
- 四 demo 行为无回归（启动、交互、resize、关闭）。
- 不改变线程模型与关闭握手语义。
- M1 涉及的注释与契约文档已同步更新。
- `AppPlugin` 契约中无独立 prepare hook，prepare 归属 runtime 明确。

### Milestone M2：FrameRuntime 封装化 + render loop 单入口

**目标**
- 让 `FrameRuntime` 成为唯一帧编排入口，render loop 不再越级访问 runtime 内部状态。

**包含任务**
- 2.1 / 2.2 / 2.3 / 2.4 / 2.5

**完成门槛（Gate）**
- phase 顺序与当前实现保持一致。
- resize/out-of-date 重建仍走单一入口。
- 帧节流决策点唯一化，不存在重复判定路径。
- runtime 相关注释与模块文档已更新为单入口语义。

### Milestone M3：四个 demo 迁移到新上下文契约

**目标**
- 业务层不再依赖 `Renderer` 内部字段布局。

**包含任务**
- 3.1 / 3.2 / 3.3 / 3.4 / 3.5

**完成门槛（Gate）**
- `triangle` / `rt-cornell` / `rt-sponza` / `shader-toy` 全部迁移完成。
- demo 功能与视觉体验无明显回退。
- 新增能力访问点全部通过上下文接口暴露。

### Milestone M4：crate 物理拆分（app-api + frame-runtime）

**目标**
- 把已收敛的逻辑边界映射为真实 crate 边界。

**包含任务**
- 4.1 / 4.2 / 4.3 / 4.4 / 4.5

**完成门槛（Gate）**
- `truvis-app-api` 与 `truvis-frame-runtime` 拆分完成。
- workspace 依赖图保持 DAG（无跨层回边）。
- `truvis-winit-app` 默认路径切换到新依赖图。
- 需要重命名的文件/模块已完成迁移并提供兼容导入路径。

### Milestone M5：render-passes 迁移 + 兼容层移除

**目标**
- 迁出 `render_pipeline/*` 并完成兼容窗口收口。

**包含任务**
- 5.1 / 5.2 / 5.3 / 5.4 / 5.5

**完成门槛（Gate）**
- `truvis-render-passes` 承接通用 pass。
- 旧接口全部下线（或在此里程碑完成物理移除）。
- 文档与 OpenSpec 描述全面对齐。
- 过期注释与过渡命名已清理完成。
- `GuiRgPass` 分层保持与 `gui-pass-separation` 一致（应用集成层）。

## 1. AppPlugin 上下文类型化（M1）

- [x] 1.1 定义阶段化上下文类型（`InitCtx` / `UiCtx` / `UpdateCtx` / `RenderCtx` / `ResizeCtx`，`PrepareCtx` 如保留则仅供 runtime 内部使用）
- [x] 1.2 为上下文提供稳定最小能力面，禁止透传 `Renderer` 内部可变布局
- [x] 1.3 重定义 `AppPlugin` hook 签名为上下文参数
- [x] 1.4 提供旧接口兼容桥接（deprecated）并标注迁移路径
- [x] 1.5 更新相关设计/注释，明确 hook 顺序与每阶段能力边界
- [x] 1.6 明确并落实：prepare 阶段不作为 `AppPlugin` 独立 hook，对外仅保留 runtime 侧职责

## 2. FrameRuntime 封装化（M2）

- [x] 2.1 收敛 `FrameRuntime` 对外字段可见性（优先去除非必要 `pub`）
- [x] 2.2 新增 runtime API：输入灌入、resize 判定、单帧推进等单入口能力
- [x] 2.3 `render_loop` 改为仅通过 runtime API 驱动，不直接操作 runtime 内部字段
- [x] 2.4 统一 `time_to_render` 判定点，移除重复节流检查
- [x] 2.5 保留关键 phase 观测点（tracy span / 日志）用于行为回归对比
- [x] 2.6 更新 runtime 与 render loop 的注释，明确“单入口驱动”约束

## 3. Demo 迁移（M3）

- [x] 3.1 迁移 `triangle` 到上下文化插件接口
- [x] 3.2 迁移 `rt-cornell` 到上下文化插件接口
- [x] 3.3 迁移 `rt-sponza` 到上下文化插件接口
- [x] 3.4 迁移 `shader-toy` 到上下文化插件接口
- [x] 3.5 清理 demo 代码中对 `Renderer` 内部字段布局的直接依赖

## 4. crate 拆分（M4）

- [x] 4.1 新建 `truvis-app-api` 并迁移 `AppPlugin`、上下文类型、overlay 合约
- [x] 4.2 新建 `truvis-frame-runtime` 并迁移 `FrameRuntime` 与 phase 编排实现
- [x] 4.3 调整 `truvis-winit-app` 与 demo 对新 crate 的依赖与导入路径
- [x] 4.4 通过 re-export 维持过渡期兼容编译路径
- [x] 4.5 更新 workspace 清单与模块 README 的边界描述
- [x] 4.6 执行语义命名重命名（必要的文件/模块名调整）并修正引用路径
- [x] 4.7 迁移模块时执行“move + shim”策略，禁止复制同等实现长期并行

## 5. Pass 迁移与兼容层下线（M5）

- [x] 5.1 新建 `truvis-render-passes` 并迁移 `render_pipeline/*` 通用实现
- [x] 5.2 demo 调整为依赖 `truvis-render-passes`
- [x] 5.3 移除旧兼容接口：`OuterApp` / `LegacyOuterAppAdapter` / `RenderApp` / `WinitApp::run`
- [x] 5.4 更新 `README` / `ARCHITECTURE` / 模块文档到最终结构
- [x] 5.5 完成全量回归验证记录并标记兼容窗口结束
- [x] 5.6 清理过期注释与兼容迁移说明，保留最终边界注释
- [x] 5.7 明确 `GuiRgPass` 保持应用集成层，不迁入 gui-backend

## 6. 验证与文档（每个里程碑结束都执行）

- [x] 6.1 运行 `openspec validate runtime-api-crate-split --strict`
- [x] 6.2 运行 `cargo check --all`（必要时 `cargo build --all`）
- [x] 6.3 四 demo 手工回归：启动、交互、resize、关闭
- [x] 6.4 核对 `render-threading` 规范不变量未被破坏
- [x] 6.5 更新 proposal/design/tasks/spec 中里程碑完成状态与证据记录
- [x] 6.6 检查文档、注释、文件命名与当前实现一致（无语义漂移）
- [x] 6.7 交叉核对 `gui-pass-separation` / `render-context-split` 未被新实现回退
