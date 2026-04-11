## 1. 清理 truvis-logs 幽灵依赖

- [x] 1.1 从 `engine/crates/truvis-logs/Cargo.toml` 删除 `reqwest`、`serde`、`zip`、`toml`、`anyhow` 五行
- [x] 1.2 运行 `cargo check -p truvis-logs` 确认编译通过

## 2. RenderContext 从 render-graph 搬迁到 renderer

- [x] 2.1 在 `truvis-renderer/src/` 下创建 `render_context.rs`，将 `truvis-render-graph/src/render_context.rs` 中的 `RenderContext` 和 `RenderContext2` 完整搬入（含 `FifBuffers` 等所需 import）
- [x] 2.2 在 `truvis-renderer/src/lib.rs` 中添加 `pub mod render_context;`
- [x] 2.3 确保 `truvis-renderer/Cargo.toml` 已有所需依赖（`truvis-scene`、`truvis-asset` 已存在；确认 `truvis-render-graph` 中 `FifBuffers` 的可访问性）
- [x] 2.4 更新 `truvis-renderer/src/renderer.rs` 中的 import：从 `truvis_render_graph::render_context` 改为 `crate::render_context`
- [x] 2.5 更新 `truvis-app` 中所有引用 `truvis_render_graph::render_context::RenderContext` 的文件（约 10 个），改为 `truvis_renderer::render_context::RenderContext`
- [x] 2.6 更新 `truvis-gui-backend/src/gui_pass.rs` 中的 import（临时步骤，task 4 会彻底处理——gui-backend 无法依赖 renderer，故改为 task 4 中重构 GuiPass::draw 签名）

## 3. ComputePass::exec 去除 RenderContext 依赖

- [x] 3.1 修改 `truvis-render-graph/src/compute_pass.rs` 中 `ComputePass::exec` 的签名：将 `render_context: &RenderContext` 替换为 `frame_label: FrameLabel, global_descriptor_sets: &GlobalDescriptorSets`，并更新函数体
- [x] 3.2 删除 `truvis-render-graph/src/render_context.rs` 文件（已搬到 renderer）
- [x] 3.3 从 `truvis-render-graph/src/lib.rs` 移除 `pub mod render_context;`
- [x] 3.4 从 `truvis-render-graph/Cargo.toml` 移除 `truvis-scene` 和 `truvis-asset` 依赖
- [x] 3.5 更新 `truvis-app` 中所有 `ComputePass::exec` 的调用点（accum_pass、blit_pass、sdr_pass、denoise_accum_pass），提取 frame_label 和 global_descriptor_sets
- [x] 3.6 运行 `cargo check -p truvis-render-graph` 确认编译通过

## 4. GuiRgPass 从 gui-backend 搬迁到 truvis-app

- [x] 4.1 在 `truvis-app/src/` 下创建 `gui_rg_pass.rs`，将 `truvis-gui-backend/src/gui_pass.rs` 中 `GuiRgPass` 结构体和 `impl RgPass for GuiRgPass` 搬入
- [x] 4.2 在 `truvis-app/src/lib.rs` 中添加 `pub mod gui_rg_pass;`
- [x] 4.3 从 `truvis-gui-backend/src/gui_pass.rs` 中移除 `GuiRgPass` 和 `impl RgPass for GuiRgPass`，以及不再需要的 render-graph import；同时重构 `GuiPass::draw` 签名移除 `&RenderContext` 改为显式参数
- [x] 4.4 从 `truvis-gui-backend/Cargo.toml` 中移除 `truvis-render-graph` 依赖（保留 `truvis-utils`，因 `enumed_map!` 宏仍在使用）
- [x] 4.5 更新 `truvis-app` 中所有 `use truvis_gui_backend::gui_pass::GuiRgPass` 的 import，改为 `crate::gui_rg_pass::GuiRgPass`
- [x] 4.6 运行 `cargo check -p truvis-gui-backend` 确认编译通过

## 5. 全量验证

- [x] 5.1 运行 `cargo check --workspace` 确认整个 workspace 编译通过
- [x] 5.2 验证依赖图：确认 render-graph 不依赖 scene/asset，gui-backend 不依赖 render-graph，logs 无幽灵依赖
