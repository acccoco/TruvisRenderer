## Context

当前架构采用 `FrameRuntime` + `FramePlugin` 模型：`FrameRuntime` 拥有唯一的 `Box<dyn FramePlugin>`，硬编码 `GuiHost`、`CameraController`、`OverlayModule` vec，并在 `run_frame` 中固定编排 GUI new_frame / compile / submit_gui_data 流程。

这导致：
- GUI 是 runtime infra 而非可选能力——即使 app 不需要 GUI，`RenderCtx` 仍携带 `gui_draw_data`
- Plugin 只有一个，无法框架层面复用能力单元
- 每个 app 在 render graph 中手动接入 `GuiRgPass`，与 runtime 层的 GUI 编排形成双重管理
- Camera 和 Overlay 绑定在 runtime 里，不同 app 无法灵活定制

已有 specs（`frame-runtime-boundary`、`runtime-api-boundary`、`layered-frame-orchestration`、`render-backend-lifecycle-ctx`）定义了 RenderBackend 生命周期方法和 Ctx 类型系统，这些在本次改动中保持不变——变的是谁拥有 Plugin、谁编排 Plugin 调用、GUI 归属。

## Goals / Non-Goals

**Goals:**
- 引入统一 `Plugin` trait，定义可复用能力单元的生命周期契约
- 用 `BaseApp`（帧骨架）+ `FrameApp`（App 契约）替代 `FrameRuntime` + `FramePlugin`
- GUI 成为普通 Plugin，App 自主决定是否引入和如何编排
- App 持有 Plugin，在 hook 点中以具体类型直接编排（解决 Plugin 间通信）
- RenderGraph 由 App 在 render hook 中构建，消除 runtime 层对 graph 结构的预设
- Camera / Overlay 从 runtime 移入 App 的 Plugin 集合

**Non-Goals:**
- 不设计 Plugin 自动发现或注册表机制（App 显式持有即可）
- 不引入 Plugin 间消息总线或事件系统（App 作为中介直接调用）
- 不改变 RenderBackend 的 Ctx 类型系统（`RenderBackendUpdateCtx`、`RenderBackendRenderCtx` 等保持现有设计）
- 不改变线程模型（仍然是 winit 主线程 + 渲染线程）
- 不在本次实现多级 Plugin 嵌套（单层 Plugin 足够当前需求）

## Decisions

### D1: 统一 Plugin trait，生命周期 + 特有方法分离

**选择**：定义一个 `Plugin` trait 覆盖标准生命周期（init / on_input / update / on_resize / shutdown），所有 hook 有默认空实现。Plugin 的特有能力（如 `GuiPlugin::ui()`、`RtPipeline::contribute_passes()`）作为具体类型的方法暴露。

**理由**：
- 统一 trait 让 App 可以用 `for_each_plugin(|p| p.init(ctx))` 批量管理生命周期
- 特有方法通过具体类型调用，保持类型安全，避免 downcast
- 拒绝了"无统一 trait，全靠具体类型"的方案——缺少生命周期规范
- 拒绝了"大 trait 包含所有能力（含 contribute_passes / build_ui）"的方案——不是每个 Plugin 都需要所有能力，trait 会膨胀

### D2: BaseApp 拥有 RenderBackend 和输入队列，通过 FrameAppHooks 回调 App

**选择**：`BaseApp` struct 持有 `RenderBackend` + 待处理 `InputEvent` 队列，提供 `run_frame(&mut self, app: &mut impl FrameAppHooks)` 方法，固定帧骨架顺序（begin_frame → on_input → update → prepare → render → present → end_frame），在变化点回调 App。

App 通过 composition 持有 `BaseApp`（`Option<BaseApp>`），在 `FrameApp::run_frame` 中取出 base 调用 `base.run_frame(self)`，调用结束后放回。

**Borrow 冲突解决**：`BaseApp` 用 `Option` 包裹，`run_frame` 时 `.take()` 取出，执行完放回。这是标准 Rust 模式，开销可忽略。

**理由**：
- 不变的帧骨架代码只写一次，避免每个 App 重复
- App 在 hook 里自由编排 Plugin，BaseApp 不知道 Plugin 的存在
- 输入事件先由 BaseApp 缓存，再在 `on_input` 中交给 App；输入状态如何累计、GUI 如何消费、Camera 如何响应都由 App/Plugin 决定
- `&mut impl FrameAppHooks` 使用静态分发，无虚表开销
- 拒绝了"BaseApp 和 App 平级由 render_loop 持有"的方案——App 不自包含，测试不便
- 拒绝了"完全消除 BaseApp，App 自己调 RenderBackend 方法"的方案——帧骨架重复

### D3: FrameApp 和 FrameAppHooks 分为两个 trait

**选择**：

- `FrameApp`：面向 render_loop 的外部契约（init / run_frame / on_resize / shutdown）
- `FrameAppHooks`：面向 BaseApp 的内部 hook 点（on_input / update / render / camera）

**理由**：
- 职责分离：render_loop 只看到 `FrameApp`，BaseApp 只看到 `FrameAppHooks`
- `run_frame` 在 `FrameApp` 上而非 `FrameAppHooks` 上——App 可以在调用 `base.run_frame(self)` 前后做额外工作
- 两个 trait 可以合并为一个（所有方法放一起），但分离后语义更清晰

### D4: GUI 完全提取为 GuiPlugin

**选择**：创建 `GuiPlugin` struct，封装：
- `imgui::Context`（现在 `GuiHost` 持有的）
- Input forwarding（`handle_input`）
- Font 初始化（`init_font`）
- Frame management（`begin_frame` / `ui()` / `end_frame` / `compile`）
- GPU 资源管理（现在 `GuiBackend` 的 `GuiMesh` / `tex_map`）
- Render graph pass 贡献（`contribute_passes`，封装现在 app 手动做的 `GuiRgPass`）

**App 编排 GUI 的方式**：
```
app.update():
  gui.begin_frame(dt)
  scene.build_ui(gui.ui())    // 特有方法
  rt.build_ui(gui.ui())       // 特有方法
  gui.end_frame()

app.render():
  graph = RenderGraphBuilder::new()
  gui.prepare_render_data(render_ctx)
  rt.contribute_passes(&mut graph)
  gui.contribute_passes(&mut graph)  // GUI pass 最后
  graph.compile().execute()
```

**GuiPlugin 的 init 需要特殊处理**：font texture 注册需要 `RenderWorld` 的 `BindlessManager`，这通过 `PluginInitCtx` 提供。

**GuiBackend 从 RenderPresent 剥离**：当前 `RenderPresent` 直接持有 `pub gui_backend: GuiBackend`，`RenderBackend` 通过 `render_present.gui_backend` 调用 `submit_gui_data` 和 `register_gui_font`。GuiPlugin 接管 GuiBackend 后：
- `RenderPresent` 移除 `gui_backend` 字段
- `RenderBackend::submit_gui_data` 和 `RenderBackend::register_gui_font` 方法移除
- `GuiPlugin` 在 init 中自行创建 `GuiBackend`（或等价结构），利用 `PluginInitCtx` 访问 `BindlessManager` 和 `GfxResourceManager`
- `GuiPlugin` 在 render hook 内通过 `prepare_render_data(&mut self, ctx: &PluginRenderCtx)` 上传当前 frame 的 mesh 数据，不经过 RenderBackend；随后 `contribute_passes(&self, ...)` 只把已准备好的 mesh/texture map 接入 RenderGraph

**理由**：
- GUI 不再泄漏到 runtime 层（`RenderCtx` 不含 `gui_draw_data`）
- 不需要 GUI 的 App 不引入 `GuiPlugin` 即可
- App 可以完全控制 GUI 编排顺序和 UI 内容
- mesh 上传需要修改 per-frame buffer，因此 render hook 必须允许 App 以 `&mut self` 访问 `GuiPlugin`
- 拒绝了"GUI 作为 runtime infra，Plugin 只通过 build_ui hook 画 UI"的方案——GUI 横跨 input/update/render 多阶段，强行作为 infra 导致 runtime 对 GUI 有特殊逻辑，不够干净

### D5: PluginCtx 类型由 Plugin 层定义

**选择**：在 `truvis-frame-api` 中定义 Plugin 层面的 Ctx 类型：

- `PluginInitCtx`：包含 `&mut World`、`&mut RenderWorld`、`&mut CmdAllocator`、swapchain info、`&RenderPresent`
- `PluginUpdateCtx`：包含 `&mut World`、`&mut PipelineSettings`、`&FrameSettings`、`delta_time_s`
- `PluginRenderCtx`：包含 `&RenderWorld`、`&RenderPresent`、`timeline`（不含 `gui_draw_data`）
- `PluginResizeCtx`：包含 `&mut RenderWorld`、`&RenderPresent`

这些从 RenderBackend Ctx 裁剪而来，由 App 在 hook 中构造后传给 Plugin。

**理由**：
- Plugin 不直接接触 RenderBackend Ctx，保持分层
- `gui_draw_data` 从 `RenderCtx` 移除——GUI 相关数据由 `GuiPlugin` 自己管理
- App 可以决定给不同 Plugin 暴露不同的 Ctx 子集

### D6: Overlay 合并入 Plugin 体系

**选择**：现有 `OverlayModule` trait 废除。`DebugInfoOverlay` 和 `PipelineControlsOverlay` 改为实现 `Plugin` trait，App 自行持有并在 `build_ui` 阶段调用。

**Overlay 的额外数据需求**：当前 `OverlayContext` 包含 `camera`、`swapchain_extent`、`accum_frames_num`、`pipeline_settings`、`delta_time_s`。其中 `pipeline_settings`、`delta_time_s` 可通过 `PluginUpdateCtx` 获取；但 `camera`、`swapchain_extent`、`accum_frames_num` 不在 `PluginUpdateCtx` 中。

解决方式：Overlay 作为 Plugin，其 `Plugin::update` 处理 `PluginUpdateCtx` 中的数据。对于 camera 等额外数据，Overlay struct 暴露特有方法（如 `build_overlay_ui(ui, camera, swapchain_extent, accum_frames)`），由 App 在 GUI 帧内直接调用并传入 App 持有的 camera。App 知道所有具体类型，这和其他 Plugin 的特有方法调用模式一致。

**理由**：
- Overlay 只是"只关心 UI 的 Plugin"，没必要保持独立的 trait 和注册机制
- App 对 overlay 的显示有完全控制权
- camera 等数据本就由 App 持有，由 App 传递给 overlay 的特有方法是自然的

### D7: 特定渲染管线作为 App 持有的具体 Plugin

**选择**：Triangle、ShaderToy、RT Pipeline 等渲染能力拆为具体 Plugin。Plugin 实现标准生命周期（init / update / on_resize / shutdown），并用特有方法暴露渲染图贡献能力，例如：

```
triangle.contribute_passes(&mut graph, &render_ctx)
shader_toy.contribute_passes(&mut graph, &render_ctx)
rt_pipeline.contribute_passes(&mut graph, &render_ctx)
```

App 在 `FrameAppHooks::render(&mut self, ctx)` 中创建 RenderGraph，按自身策略调用各 Plugin 的贡献方法并执行 graph。Plugin 负责持有 pipeline/pass/GPU 资源，App 负责决定插件组合与 pass 顺序。

**理由**：
- App 是编排层，Plugin 是可复用能力单元；这正好消除单一 `FramePlugin` 只能包一整个 demo 的限制
- 渲染管线之间的 pass 拓扑差异较大，不应塞进统一 `Plugin` trait
- 用具体类型方法保持类型安全，避免 downcast 或字符串注册表

### D8: FrameApp shutdown 使用可对象化签名

**选择**：`FrameApp::shutdown(&mut self)` 作为 render loop 的关闭入口。App 在该方法中先调用各 Plugin 的 `shutdown()`，再取出 `BaseApp` 执行 `destroy(self)`。

**理由**：
- render loop 持有 `Box<dyn FrameApp>`，`shutdown(self)` 不是合适的 trait object 调用形态
- `&mut self` 允许 App 通过 `Option<BaseApp>::take()` 消费 BaseApp，同时保留统一的对象安全入口

### D9: RenderBackend 的 World 所有权本次保持不变

**选择**：本 change 不迁移 `World` / `AssetHub` / `RenderWorld` 的所有权。`RenderBackendUpdateCtx`、`RenderBackendInitCtx`、`RenderBackendResizeCtx` 仍由 RenderBackend 产出，App 只在 hook 中裁剪出 Plugin Ctx。

**理由**：
- 本次目标是移除 `FrameRuntime` 和 GUI/渲染管线硬编码，而不是重做 RenderBackend/World 边界
- 把 `World` 所有权也迁出 RenderBackend 会扩大变更面，和 Plugin/BaseApp 主线耦合过重
- 后续如果要做 ECS/schedule 或 asset plugin 化，应另开 change 处理

## Risks / Trade-offs

**[App boilerplate 增加]** → 每个 App 需要实现 `FrameApp` + `FrameAppHooks`，编排 Plugin 调用。Mitigation：提供 `for_each_plugin` 模式和文档化的常用编排模板；GUI 编排只需 3-4 行。

**[Option<BaseApp> take/put 模式]** → `self.base.take()` + `self.base = Some(base)` 在 `run_frame` 中有一瞬间 `base` 为 `None`。Mitigation：这是 `run_frame` 内部实现细节，不会被外部观察到；如果 hook panic，base 丢失，但 panic 后 app 本身也不可恢复。

**[Plugin trait 可能过于简单]** → 当前 Plugin trait 只有生命周期 hook，没有 `contribute_passes` 或 `build_ui`。如果未来大多数 Plugin 都需要这些，可能需要 extension trait。Mitigation：先保持简单，如果模式稳定再引入 `RgContributor` / `UiContributor` 等 extension trait。

**[迁移范围]** → 四个 demo app + FrameRuntime + FramePlugin + GuiHost + Overlay 都要改。Mitigation：分阶段迁移，先建立新架构的骨架，再逐个迁移 app。
