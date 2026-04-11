## Context

`engine/crates/` 中的 crate 依赖图存在三处层次违反和一处无用依赖：

1. **`truvis-render-graph` → `truvis-scene` + `truvis-asset`**：原因是 `RenderContext` 定义在 render-graph 中，它是一个 "God struct"，聚合了渲染期间的所有状态（包括 scene/asset），导致 render-graph 被迫知道 scene 和 asset 的类型。
2. **`truvis-gui-backend` → `truvis-render-graph`**：原因是 `GuiRgPass`（render graph 适配器）定义在 gui-backend 中，它 `impl RgPass` 并持有 `&RenderContext`。
3. **`truvis-logs` 的幽灵依赖**：`reqwest`/`serde`/`zip`/`toml`/`anyhow` 在 Cargo.toml 中声明但源码中完全未使用。

当前 `RenderContext` 被 10+ 个文件引用（renderer、app 中的各种 pass、gui-backend），迁移需要更新所有 import 路径。

## Goals / Non-Goals

**Goals:**
- render-graph 成为纯粹的 pass 编排层，不依赖 scene/asset
- gui-backend 成为纯粹的 ImGui Vulkan 录制层，不依赖 render-graph
- 消除 truvis-logs 的无用依赖
- 保持所有现有功能不变

**Non-Goals:**
- 不重构 `OuterApp` trait 签名（`GfxSemaphore` → renderer 层类型）— 这涉及更大的 API 设计讨论，留到后续 change
- 不拆分 truvis-app 中的 render_pipeline/ 和示例代码 — 这是独立的组织问题
- 不拆分 truvis-shader-binding — 编译扇出问题可容忍，且拆分成本高

## Decisions

### Decision 1: RenderContext 整体搬迁到 truvis-renderer（而非拆分为两个 struct）

**选择**：将 `RenderContext` 和 `RenderContext2` 原样从 `truvis-render-graph` 搬到 `truvis-renderer`，render-graph 内部的 `ComputePass::exec` 改为只接收它实际需要的参数。

**备选方案**：
- *方案 B：拆分为 RgContext + RenderContext*：render-graph 定义一个精简的 `RgContext`（只含 frame_counter、global_descriptor_sets 等），renderer 组装完整的 `RenderContext` 包含 `RgContext` 作为字段。
- *方案 C：trait 抽象*：render-graph 定义 `trait RenderContextProvider`，renderer 的 `RenderContext` 实现它。

**理由**：
- `ComputePass::exec` 是 render-graph 内部唯一使用 `RenderContext` 的代码，且只用到 `frame_counter` 和 `global_descriptor_sets` 两个字段。直接改参数列表比引入新类型更简单。
- 方案 B 引入了一个新类型和嵌套关系，增加了复杂度却没有明显收益 — render-graph 中的其他代码（render_graph/ 子目录下的 pass/executor/graph）完全不使用 RenderContext。
- 方案 C 过度抽象，增加 trait dispatch 和泛型复杂度。
- 搬迁后，render-graph 的 Cargo.toml 可以直接删掉 `truvis-scene` 和 `truvis-asset` 依赖。

### Decision 2: GuiRgPass 搬到 truvis-app（而非 truvis-renderer）

**选择**：将 `GuiRgPass` 从 `truvis-gui-backend/gui_pass.rs` 搬到 `truvis-app`。

**备选方案**：
- *搬到 truvis-renderer*：renderer 是更"正统"的位置，因为它负责组装 render graph。

**理由**：
- `GuiRgPass` 的当前使用者全部在 `truvis-app`（`triangle_app.rs`、`shader_toy_app.rs`、`rt_render_graph.rs`），没有 `truvis-renderer` 中的代码直接使用它。
- 搬到 renderer 会让 renderer 依赖 gui-backend（目前已有此依赖），但实际组装 GUI pass 进 render graph 的代码全在 app 层。跟随使用者更自然。
- `GuiPass`（纯 Vulkan 录制）保留在 gui-backend，保持 gui-backend 的纯粹性。

### Decision 3: truvis-logs 直接删除未使用依赖

**选择**：删除 Cargo.toml 中的 5 行：`reqwest`、`serde`、`zip`、`toml`、`anyhow`。

**理由**：经过搜索，整个 `truvis-logs/src/` 只有一个 40 行的 `lib.rs`，只用了 `log`、`env_logger`、`anstyle`、`chrono`。这 5 个依赖完全是幽灵依赖，可能是从其他 crate 复制 Cargo.toml 时残留的。

## Risks / Trade-offs

- **import 路径变更的广泛影响** → 迁移 `RenderContext` 后需更新 10+ 个文件的 import 路径（从 `truvis_render_graph::render_context` 改为 `truvis_renderer::render_context`）。风险低：纯机械替换，编译器会捕获所有遗漏。
- **`ComputePass::exec` 签名变化** → 从 `&RenderContext` 改为具体参数（`&FrameCounter`, `&GlobalDescriptorSets`）。所有调用点需适配。调用点集中在 truvis-app 的几个 compute pass 中（accum、blit、sdr、denoise_accum），数量可控。
- **`GuiRgPass` 搬迁可能引入循环依赖** → 需要确认 truvis-app 依赖 truvis-gui-backend（已有）且 gui-backend 不依赖 app（正确）。搬迁后 app 从 gui-backend 导入 `GuiPass`，从自身定义 `GuiRgPass`，无循环风险。
