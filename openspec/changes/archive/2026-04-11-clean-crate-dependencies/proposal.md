## Why

`engine/crates/` 下的 crate 存在多处依赖层次违反：`truvis-render-graph` 向上依赖了 `truvis-scene` 和 `truvis-asset`，`truvis-gui-backend` 依赖了 `truvis-render-graph`，`truvis-logs` 声明了 5 个完全未使用的依赖（包括 `reqwest`，拖慢编译）。这些问题导致 render-graph 无法作为纯粹的 pass 编排层独立使用，gui-backend 的职责边界模糊，且编译依赖图中存在不必要的耦合。现在清理可以为后续的引擎扩展（如 Tauri 集成、新的渲染管线）建立干净的分层基础。

## What Changes

- **`truvis-render-graph`**: 移除对 `truvis-scene` 和 `truvis-asset` 的依赖。将 `RenderContext` 拆分为两层：render-graph 拥有纯渲染上下文（`RgContext`），完整的 `RenderContext`（含 scene/asset）由 `truvis-renderer` 组装。
- **`truvis-gui-backend`**: 移除对 `truvis-render-graph` 的依赖。将 `GuiRgPass`（render graph 适配器）从 gui-backend 迁移到 `truvis-renderer` 或 `truvis-app`，gui-backend 只保留纯 Vulkan 录制的 `GuiPass`。
- **`truvis-logs`**: 删除 5 个未使用的幽灵依赖（`reqwest`、`serde`、`zip`、`toml`、`anyhow`）。
- **`truvis-app`**: `OuterApp::draw` 签名中的 `GfxSemaphore` 替换为 renderer 层类型，减少 app 层对 `truvis-gfx` 的直接依赖。**BREAKING**: `OuterApp` trait 签名变化。

## Capabilities

### New Capabilities
- `render-context-split`: 将 RenderContext 拆分为 render-graph 层的 RgContext 和 renderer 层的完整 RenderContext，实现层次解耦
- `gui-pass-separation`: 将 GuiRgPass（render graph 适配）与 GuiPass（纯 Vulkan 录制）分离到不同 crate

### Modified Capabilities

## Impact

- **Crate 依赖图**: render-graph 不再依赖 scene/asset，gui-backend 不再依赖 render-graph，logs 编译更快
- **Public API**: `OuterApp` trait 签名变化（`GfxSemaphore` → renderer 层类型），`RenderContext` 拆分为 `RgContext` + `RenderContext`
- **下游代码**: 所有 `OuterApp` 实现需要适配新签名；所有引用 `truvis_render_graph::render_context::RenderContext` 的代码需改为引用 `truvis_renderer` 的版本；所有使用 `truvis_gui_backend::gui_pass::GuiRgPass` 的代码需改为新位置的导入
- **编译性能**: 移除 truvis-logs 的 reqwest 等依赖可显著减少依赖树大小
