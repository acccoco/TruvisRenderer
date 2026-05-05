## Context

经过 `split-render-context-world-renderworld` 变更，World（CPU 场景状态）和 RenderWorld（GPU 渲染状态）已物理分离。但 Renderer 仍同时持有两者，FrameRuntime 通过直接字段访问（`self.renderer.world`、`self.renderer.render_world`）来构造 Plugin 的 typed context。

当前调用模式（简化）：

```rust
// FrameRuntime::phase_update
self.renderer.update_frame_settings();      // &mut self method
self.renderer.acquire_image();              // &mut self method
// ... 然后穿透 renderer 构造 ctx:
let ctx = UpdateCtx {
    world: &mut self.renderer.world,
    pipeline_settings: &mut self.renderer.render_world.pipeline_settings,
    ...
};
self.plugin.update(&mut ctx);
```

这种"方法调用 → 字段穿透"的混合模式导致封装泄漏和隐式时序约束。

本设计建立三层（App → FrameRuntime → Renderer）的生命周期边界：每层只关注自己的生命周期，在关键节点产出 Ctx 供外层接入，不预设消费者。

## Goals / Non-Goals

**Goals:**
- Renderer 以生命周期方法 + Ctx 返回值对外暴露状态，消除 FrameRuntime 对其内部字段的直接访问
- `render_phase()` 为 `&self`（只读），在类型层面表达 render 阶段的不可变语义
- FrameRuntime 通过 Renderer 的 Ctx 返回值获取状态，不再穿透封装
- Ctx 只包含产出层自身拥有的数据；跨层数据（如 gui_draw_data）由外层组合
- 三层各自演化互不干扰：GPU 内部重构不影响 FrameRuntime，新增 Plugin hook 不影响 Renderer
- 所有 demo app 无功能回归

**Non-Goals:**
- 不将 Camera / Input / GuiHost 从 FrameRuntime 解耦为独立子系统（后续 P2）
- 不引入 tick system 或多 Plugin 支持（后续 P3，但设计应不阻塞）
- 不改变渲染线程模型（仍为 main thread + render thread）
- 不引入 trait 抽象 Renderer（不做 mock renderer）
- 不改变 RenderWorld 的 plain struct 设计
- 不引入 ECS、TypeMap 或其他通用容器

## Decisions

### Decision 1: Renderer 的 Ctx 产出模式

**选择**: Renderer 在帧生命周期的关键节点提供方法，返回 lifetime-bound 的 Ctx struct。Ctx struct 借用 Renderer 内部字段。Ctx 被 drop 后，Renderer 恢复可调用状态。

**生命周期方法签名**:

```rust
impl Renderer {
    pub fn begin_frame(&mut self);
    pub fn update_phase(&mut self) -> RendererUpdateCtx<'_>;
    pub fn submit_gui_data(&mut self, draw_data: &imgui::DrawData);
    pub fn prepare(&mut self, camera: &Camera);
    pub fn render_phase(&self) -> RendererRenderCtx<'_>;
    pub fn present(&mut self);
    pub fn end_frame(&mut self);
}
```

**理由**: Rust 方法体内 disjoint field borrowing 允许 `update_phase()` 在内部做准备工作后返回子字段的借用。调用者（FrameRuntime）拿到 Ctx 后，可同时访问自己的兄弟字段（plugin、gui_host 等），不受 renderer borrow 影响。Ctx 的生命周期由 Rust 编译器通过 block scope 自动强制。

**备选方案**:
- Renderer 接受回调（callback inversion）：Renderer 需要知道 AppHost/Plugin 接口，违反"不预设消费者"原则
- Renderer 返回 split struct（`RendererParts`）：语义不清，失去生命周期节点的概念
- 全部平坦化到 FrameRuntime：FrameRuntime 膨胀为 god struct，失去 Renderer 的封装和复用性

### Decision 2: render_phase() 使用 &self

**选择**: `render_phase()` 方法签名为 `pub fn render_phase(&self) -> RendererRenderCtx<'_>`。

**理由**: Render 阶段 Plugin 只读取 RenderWorld 来录制 GPU 命令，不修改任何 Renderer 状态。使用 `&self` 在类型层面表达这一约束。同时，共享借允许 FrameRuntime 在 render Ctx 存活期间并发读取 Renderer 的其他只读字段（虽然当前不需要）。

**前置条件**: `prepare()` 必须在 `render_phase()` 前完成所有 GPU 数据上传——这通过顺序调用自然保证。

### Decision 3: Ctx 类型定义位置

**选择**: `RendererUpdateCtx`、`RendererRenderCtx`、`RendererInitCtx`、`RendererResizeCtx` 定义在 `truvis-renderer` crate 中。

**理由**: 这些 Ctx 的字段直接引用 Renderer 内部类型（World、RenderWorld、RenderPresent、GfxSemaphore 等）。定义在 renderer crate 中，与它们借用的数据处于同一 crate，避免循环依赖。

**Plugin 侧 Ctx 处理**: Plugin trait 的方法签名使用这些 Renderer Ctx 类型（或 FrameRuntime 的组合类型）。`truvis-app-api` 依赖 `truvis-renderer` 获取 Ctx 类型定义（与当前一致）。

### Decision 4: gui_draw_data 由 FrameRuntime 组合

**选择**: Renderer 产出的 `RendererRenderCtx` 不包含 `gui_draw_data`。FrameRuntime 取得 `RendererRenderCtx` 后，组合 `gui_host.get_render_data()` 构造完整的 `RenderCtx` 传给 Plugin。

**理由**: gui_draw_data 来自 FrameRuntime 的 `gui_host`，不属于 Renderer。按"Ctx 只包含产出层数据"原则剥离。

**实现方式**:

```rust
// FrameRuntime 中
{
    let renderer_ctx = self.renderer.render_phase();  // & borrow
    let ctx = RenderCtx {
        render_world: renderer_ctx.render_world,
        render_present: renderer_ctx.render_present,
        timeline: renderer_ctx.timeline,
        gui_draw_data: self.gui_host.get_render_data(),  // sibling field
    };
    self.plugin.as_ref().unwrap().render(&ctx);
}
```

`RenderCtx`（Plugin 侧 API）保持不变，只是构造方式改为组合。

### Decision 5: submit_gui_data 作为显式注入点

**选择**: Renderer 提供 `pub fn submit_gui_data(&mut self, draw_data: &imgui::DrawData)` 方法，在 update_phase Ctx drop 之后、prepare 之前调用。

**理由**: GUI vertex/index 数据需要上传到 GPU buffer（通过 `gui_backend.prepare_render_data`），这是 GPU 操作，属于 Renderer 职责。但数据来源（imgui DrawData）由 FrameRuntime 的 gui_host 产生。显式注入方法让 Renderer 接收数据但不知道来源。

**生命周期位置**: 在 update_phase Ctx drop 之后（UI 已编译），prepare 之前（GPU 数据准备之前）。

### Decision 6: Renderer.handle_resize() 条件产出 Ctx

**选择**: `pub fn handle_resize(&mut self, new_size: [u32; 2]) -> Option<RendererResizeCtx<'_>>`。只有当 swapchain 实际重建时才返回 Ctx。

**理由**: Resize 不是每帧都发生。Renderer 内部判断是否需要重建，如果重建了则返回 Ctx 供外层（FrameRuntime）传给 Plugin 的 `on_resize` hook。如果不需要重建则返回 None。

### Decision 7: 合并 ui_phase 和 update_phase 为单一 Ctx

**选择**: 不单独提供 `ui_phase()` 方法。`update_phase()` 返回的 `RendererUpdateCtx` 包含 build_ui 所需的全部字段（pipeline_settings、accum_data、swapchain_extent、delta_time）。FrameRuntime 决定在 Ctx 存活期间先做 UI 还是先做 update。

**理由**: ui_phase 和 update_phase 需要的字段高度重叠（都需要 `&mut pipeline_settings`）。拆成两个 Ctx 只是多一次 Renderer borrow/release 循环，无语义收益。对 Renderer 而言，"外部在修改 world/settings"是同一个阶段。内部顺序决策由 FrameRuntime 承担。

### Decision 8: camera 作为 prepare() 的直接参数

**选择**: `pub fn prepare(&mut self, camera: &Camera)` 直接接受 `&Camera` 参数。

**理由**: 当前 prepare 阶段唯一的外部输入是 camera。引入 `PrepareInput` struct 属于过度抽象。等出现第二个外部输入时再引入包装。Camera 类型定义在 `truvis-renderer::platform::camera`，无额外依赖。

### Decision 9: 保留现有 update_assets 在 begin_frame 内部

**选择**: `update_assets()` 调用保留在 `begin_frame()` 方法内部（或紧随其后作为 Renderer 的内部步骤），不对外暴露。

**理由**: AssetHub 的 CPU tick 是 Renderer 的内部簿记工作（处理异步加载完成的资源）。外部无需介入或观察此过程。保持内部化减少 API surface。

## Risks / Trade-offs

**[Risk] Renderer 方法之间的调用顺序仍是隐式的** → Mitigation: 通过文档和 Ctx 的 lifetime bound 约束。Ctx 存活期间无法调用其他 Renderer 方法（编译器强制）。方法间的顺序（begin_frame 必须在 update_phase 之前）通过 API 文档和 tracy span 审计保证。未来可考虑 typestate 模式但目前 non-goal。

**[Risk] RendererUpdateCtx 暴露 &mut World 给外部可任意修改** → Mitigation: 这是有意设计——Renderer 不预设 World 如何被修改，prepare() 总是对当前 World 状态做全量同步。如果性能成为瓶颈（大场景），后续可在 World 中加 change tracking，但当前不需要。

**[Risk] Plugin 接口 breaking change** → Mitigation: `init` 签名变化（camera 独立传入）和 `RenderCtx` 的内部构造变化。由于所有 Plugin 实现都在 workspace 内（demo apps），同步更新即可。

**[Risk] run_frame 骨架方法可能变长** → Mitigation: Renderer 的纯内部操作封装在 `begin_frame`/`prepare`/`present`/`end_frame` 中。FrameRuntime 的 `run_frame` 只包含 ~15 行高层调用 + block scope，可读性良好。

**[Trade-off] submit_gui_data 是"数据注入"而非"Ctx 产出"**: 打破了 Renderer 纯"产出 Ctx"的对称模式。但 GUI 数据上传确实是 Renderer 的 GPU 操作，外部注入比 Renderer 主动获取更符合"不知道消费者"的原则。
