## Why

当前 `Renderer` 同时持有 `World` + `RenderWorld` 并提供 `&mut self` 方法，而 `FrameRuntime` 需要在调用 Renderer 方法的间隙"穿透"其内部字段来构造 typed context（`UpdateCtx`、`RenderCtx`）传给 Plugin。这造成了：

- **封装泄漏**：FrameRuntime 必须直接访问 `renderer.world`、`renderer.render_world` 等内部字段
- **隐式时序约束**：Renderer 的方法调用与字段借出不能同时发生，完全靠人工保证顺序
- **职责混淆**：FrameRuntime 既是帧编排者，又是 context 构造者，还要了解 Renderer 的内部结构
- **扩展性差**：每增加新的 phase/hook（tick system、多 plugin 等），FrameRuntime 的膨胀不可避免

经过 `split-render-context-world-renderworld` 变更后，World/RenderWorld 分离已完成，现在是建立清晰的三层生命周期边界的合适时机。

## What Changes

- **Renderer 改为"生命周期 + Ctx 产出"模式**：Renderer 的帧方法不再混合内部操作和外部状态暴露。在生命周期关键节点返回 typed Ctx（`RendererUpdateCtx`、`RendererRenderCtx`、`RendererResizeCtx`、`RendererInitCtx`），不预设消费者。
- **新增 `submit_gui_data` 注入点**：Renderer 接受外部 GUI 绘制数据（`&imgui::DrawData`），不知道数据来源。
- **`render_phase()` 改为 `&self`**：Render 阶段 Renderer 的状态只读，类型层面表达不可变语义。
- **FrameRuntime 不再直接访问 Renderer 内部字段**：所有 Renderer 状态通过其生命周期方法获取 Ctx 访问。FrameRuntime 只负责驱动 Renderer 生命周期 + 连接 Ctx 到 Plugin。
- **BREAKING**: `RenderCtx` 的 `gui_draw_data` 字段从 Renderer 侧剥离——Renderer 产出的 `RendererRenderCtx` 不含 gui_draw_data，由 FrameRuntime 在外层组合成完整的 `RenderCtx`。
- **BREAKING**: Plugin 的 `init` 签名调整——camera 不再包含在 Renderer 产出的 `RendererInitCtx` 中，由 FrameRuntime 单独传入。
- **移除 FrameRuntime 对 Renderer 字段的直接访问**（`renderer.world`、`renderer.render_world`、`renderer.timer`、`renderer.render_present` 等）。

## Capabilities

### New Capabilities
- `renderer-lifecycle-ctx`: Renderer 以生命周期方法 + Ctx 返回值的模式对外暴露状态，不预设消费者。包括 update_phase / render_phase / resize / init 四个 Ctx 产出点。
- `layered-frame-orchestration`: 三层（App-FrameRuntime-Renderer）各自管理自己的生命周期，通过 Ctx 在层间通信。外层驱动内层生命周期，内层不知道外层存在。

### Modified Capabilities
- `world-renderworld-split`: RendererUpdateCtx / RendererRenderCtx 取代原来的 UpdateCtx / RenderCtx 中直接从 Renderer 字段借出的模式。Plugin 侧的 Ctx 可能变为 Renderer Ctx 的直接复用或 FrameRuntime 层的组合。

## Impact

- **直接修改的 crate**: `truvis-renderer`（生命周期方法重构 + Ctx 类型定义）、`truvis-app-api`（Plugin trait 签名 + Ctx 类型更新）、`truvis-frame-runtime`（移除字段穿透、改用 Ctx 驱动）
- **间接影响的 crate**: `truvis-app`（demo apps 适配新的 Plugin 签名）、`truvis-render-passes`（如果 pass 直接使用 Ctx 类型）
- **新增类型**: `RendererUpdateCtx`、`RendererRenderCtx`、`RendererInitCtx`、`RendererResizeCtx`（定义在 `truvis-renderer`）
- **删除的模式**: FrameRuntime 中所有 `self.renderer.world` / `self.renderer.render_world` / `self.renderer.timer` 等直接字段访问
- **Plugin API breaking change**: `init` 签名变化、`RenderCtx` 字段来源变化
