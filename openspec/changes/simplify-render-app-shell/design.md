## Context

当前代码已经形成了两层 runtime 抽象：

```text
render loop
    |
    v
FrameApp trait
    |
    v
FrameAppShell<A: FrameAppState>
    |
    +-- BaseApp
    |     +-- RenderBackend
    |     +-- input_events
    |
    +-- app state implementing FrameAppState + FrameAppHooks
```

`BaseApp` 的原始目标是把固定帧骨架从具体 app 中抽出，避免每个 demo 重复实现 `input -> update -> prepare -> render -> present`。但 `FrameAppShell` 已经成为所有 demo 的统一入口，具体 app 不再直接持有或调用 `BaseApp`。因此 `BaseApp` 作为 public API 的边界价值下降，继续暴露会增加理解成本。

现有代码仍需要保留三个边界：

- render loop 需要一个 object-safe trait，用于持有 `Box<dyn ...>`。
- runtime shell 需要统一持有 `RenderBackend`、输入事件队列和固定帧顺序。
- 具体 app 需要持有 GUI、camera/input state、overlay、render pipeline plugin，并在 hook 中编排它们。

## Goals / Non-Goals

**Goals:**

- 将 `BaseApp` 合并进 `RenderAppShell`，移除公开 `BaseApp` API。
- 将 `FrameApp` 重命名为 `RenderApp`，强调它是渲染线程驱动的 app 契约。
- 将 `FrameAppShell` 重命名为 `RenderAppShell`。
- 将 `FrameAppState` 与 `FrameAppHooks` 合并为单一 `RenderAppHooks` trait。
- 保持帧执行顺序、resize 时序、输入事件交付、shutdown 顺序和 Vulkan 资源线程归属不变。
- 同步更新文档与 OpenSpec，避免继续出现 `BaseApp` / `FrameAppShell` 双概念描述。

**Non-Goals:**

- 不改变 Plugin trait 或 Plugin typed contexts 的能力边界。
- 不改变 RenderBackend 生命周期方法或 Ctx 字段。
- 不改变 winit 主线程 + 渲染线程的线程模型。
- 不引入 headless runner、测试 runner 或多种 shell 实现。
- 不把 GUI、camera、overlay 或具体 render pipeline plugin 放回 runtime shell。

## Decisions

### D1: `BaseApp` 不再作为 public runtime type

**选择**：`RenderAppShell` 直接持有 `RenderBackend` 与 `Vec<InputEvent>`，并在自身实现中执行固定帧骨架。

**理由**：

- 当前所有入口都通过 shell 包装具体 app，`BaseApp` 不再减少 app 侧重复代码。
- 合并后对外概念从 `FrameAppShell + BaseApp + FrameAppState + FrameAppHooks` 收敛为 `RenderAppShell + RenderAppHooks`。
- 固定帧顺序仍集中在一个实现里，不会退回每个 app 手写骨架。

**备选方案**：保留 public `BaseApp`，只重命名其它 API。拒绝原因是它继续暴露一个当前没有独立调用方的层级。

**备选方案**：保留一个 public `FrameCore` / `RenderAppCore`。拒绝原因是现阶段没有 headless runner 或多 shell 需求，公开 core 会提前固化不必要 API。

### D2: 可以使用私有 helper 保持 shell 实现清晰

**选择**：合并 public type 后，`RenderAppShell` 内部可以使用私有方法划分 `init_backend`、`run_frame_skeleton`、`handle_resize`、`destroy_backend` 等逻辑。

**理由**：

- 对外减少抽象，不等于把所有实现堆进一个大函数。
- 私有 helper 不形成 public API 承诺，后续如果出现多 runner，再从内部提取 core 成本较低。

### D3: `FrameApp` 重命名为 `RenderApp`

**选择**：render loop 面向的 object-safe trait 命名为 `RenderApp`。

**理由**：

- `FrameApp` 容易和每帧 hook 混淆；`RenderApp` 更接近它在渲染线程中的角色。
- `RenderApp` 的方法仍覆盖窗口绑定初始化、帧推进、输入灌入、swapchain 重建、节流判断和关闭。

**兼容说明**：历史兼容接口中曾有旧 `RenderApp` 名称。该旧接口保持下线；新的 `RenderApp` 是当前 `FrameApp` 的重命名，不是恢复旧兼容层。

### D4: `FrameAppState` 与 `FrameAppHooks` 合并为 `RenderAppHooks`

**选择**：具体 app 只实现一个 trait：

```rust
pub trait RenderAppHooks {
    fn init(&mut self, ctx: RenderAppInitCtx<'_>);
    fn on_input(&mut self, events: &[InputEvent]);
    fn update(&mut self, ctx: &mut RenderBackendUpdateCtx);
    fn render(&mut self, ctx: &RenderBackendRenderCtx);
    fn camera(&self) -> &Camera;
    fn on_resize(&mut self, ctx: RenderAppResizeCtx<'_>) {}
    fn shutdown(&mut self) {}
}
```

**理由**：

- `FrameAppState: FrameAppHooks` 目前只由 shell 消费，拆成两层 trait 的复用价值不足。
- `RenderAppHooks` 表达的是 shell 对具体 app 的所有回调点，包含生命周期 hook 与每帧 hook，语义完整。
- 具体 app 侧 boilerplate 从两个 impl 收敛为一个 impl。

**备选方案**：命名为 `RenderAppState`。拒绝原因是 trait 描述的是 shell 调用协议，不是 state 数据结构本身。

### D5: API rename 采用一次性破坏式迁移

**选择**：不保留 `FrameApp`、`FrameAppShell`、`FrameAppState`、`FrameAppHooks`、`BaseApp` 的兼容 re-export。

**理由**：

- 当前调用方集中在 workspace 内，迁移面可控。
- 兼容 shim 会让“减少抽象层级”的目标打折，并延长旧命名在文档和 IDE 补全中的存在时间。

如果后续发现外部用户依赖这些 crate，可以另开兼容窗口 change，而不是在本次架构收敛中预设。

## Risks / Trade-offs

**[RenderAppShell 变胖]** -> 使用私有 helper 方法保持生命周期和帧阶段边界清晰；避免引入新的 public core type。

**[RenderApp 名称与历史旧接口冲突]** -> 文档明确新的 `RenderApp` 是当前 `FrameApp` 的重命名，旧兼容接口不恢复。

**[大范围重命名导致漏改]** -> 分阶段迁移 API、demo、winit app、docs，并用 `rg` 检查旧名字残留。

**[shutdown 顺序回归]** -> tasks 中单独验证 `RenderAppHooks::shutdown` 先于 backend/Gfx destroy。

**[OpenSpec 基线仍包含历史 FrameRuntime/FramePlugin 叙述]** -> 本 change 的 spec delta 明确最终模型；若旧 completed changes 尚未 archive，实施前应确认归档顺序，避免 spec archive 冲突。

## Migration Plan

1. 在 `truvis-frame-api` 中将 `FrameApp` 改名为 `RenderApp`，将 `FrameAppHooks` 语义迁移到 runtime crate 的 `RenderAppHooks` 或保持 API crate 中定义但以新名暴露。
2. 在 `truvis-frame-runtime` 中将 `FrameAppShell` 改名为 `RenderAppShell`，合并 `BaseApp` 字段和方法。
3. 将 `FrameAppState` 与 `FrameAppHooks` 合并为 `RenderAppHooks`，迁移四个 demo app 的 impl。
4. 更新 `truvis-winit-app` 使用 `Box<dyn RenderApp>` 和 `RenderAppShell::new(...)`。
5. 删除 `BaseApp` public export 和相关文档叙述。
6. 运行格式化、编译检查和可用 demo smoke test。

## Open Questions

- `RenderAppHooks` 应定义在 `truvis-frame-api` 还是 `truvis-frame-runtime`：如果希望 app crate 只依赖 API crate 的契约，应放在 API crate；如果认为 hooks 只服务 shell，可放在 runtime crate。当前建议保留在 API crate 或由 runtime crate re-export，避免 app 层直接耦合 shell 实现细节。
