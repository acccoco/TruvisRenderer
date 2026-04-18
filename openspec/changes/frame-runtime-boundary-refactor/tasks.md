## 1. 命名与接口入口（兼容阶段）

- [ ] 1.1 在 `truvis-app` 中引入 `FrameRuntime` 命名入口，并保留 `RenderApp` 兼容导出（deprecated 注释）
- [ ] 1.2 定义 `AppPlugin` trait（单 trait 多 hook），并提供 `LegacyOuterAppAdapter`
- [ ] 1.3 更新 `truvis-winit-app` 对 runtime/app 接口的引用路径，使新旧命名可共存

## 2. FrameRuntime 阶段化重构

- [ ] 2.1 将现有 `big_update` 拆分为显式 phase 方法（input/update/prepare/render/present）
- [ ] 2.2 保证拆分前后行为顺序一致（含 GUI、resize、present）
- [ ] 2.3 在代码注释中声明每个 phase 的输入/输出与职责边界

## 3. Renderer 职责收敛

- [ ] 3.1 识别并迁出 `Renderer` 中的 world/update 触发逻辑（scene/asset 侧）
- [ ] 3.2 在 `FrameRuntime` phase 中接管上述逻辑调度
- [ ] 3.3 保持 `Renderer` 聚焦 backend 能力（device/swapchain/cmd/sync/submit/present）

## 4. 默认 UI 解耦

- [ ] 4.1 将 runtime 硬编码 overlay UI 抽离为可注册模块
- [ ] 4.2 保持现有 demo 默认显示效果不回归
- [ ] 4.3 为后续禁用/替换 overlay 留出稳定注册点

## 5. Demo 迁移与兼容收口

- [ ] 5.1 迁移 `triangle` 到 `AppPlugin` 路径
- [ ] 5.2 迁移 `rt-cornell` 到 `AppPlugin` 路径
- [ ] 5.3 迁移 `rt-sponza` 到 `AppPlugin` 路径
- [ ] 5.4 迁移 `shader-toy` 到 `AppPlugin` 路径
- [ ] 5.5 在四个 demo 验证通过后，标记旧 `OuterApp` 兼容层为待移除

## 6. 验证与文档

- [ ] 6.1 回归运行四个 demo（启动、交互、关闭）确认行为一致
- [ ] 6.2 核对 `render-thread-isolation` 的线程关闭握手未被破坏
- [ ] 6.3 更新设计文档中涉及 `RenderApp/OuterApp` 命名与职责描述
- [ ] 6.4 运行 `openspec validate frame-runtime-boundary-refactor --strict`

## 7. 后续 change 准备（非本 change 实施）

- [ ] 7.1 产出 `truvis-render-passes` 物理拆分清单（模块/依赖/迁移顺序）
- [ ] 7.2 产出 `truvis-frame-runtime` 与 `truvis-app-api` 拆分草案
