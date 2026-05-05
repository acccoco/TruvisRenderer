## 1. FifBuffers 迁移到 render-interface

- [x] 1.1 将 `fif_buffer.rs` 从 `truvis-render-graph/src/resources/` 移动到 `truvis-render-interface/src/`，更新 `truvis-render-interface` 的 `lib.rs` 模块声明
- [x] 1.2 更新 `truvis-render-graph` 的 `lib.rs`，移除 `fif_buffer` 模块声明；如需过渡，添加 deprecated re-export
- [x] 1.3 更新所有 `use truvis_render_graph::resources::fif_buffer::FifBuffers` 路径为 `truvis_render_interface::fif_buffer::FifBuffers`（涉及 `truvis-renderer`、`truvis-app`）
- [x] 1.4 `cargo check --workspace` 验证编译通过

## 2. 定义 RenderWorld

- [x] 2.1 在 `truvis-render-interface/src/` 新增 `render_world.rs`，定义 `RenderWorld` 结构体（字段：gpu_scene, bindless_manager, global_descriptor_sets, gfx_resource_manager, fif_buffers, sampler_manager, per_frame_data_buffers, frame_counter, frame_settings, pipeline_settings, delta_time_s, total_time_s, accum_data）
- [x] 2.2 在 `truvis-render-interface` 的 `lib.rs` 中添加 `pub mod render_world` 导出
- [x] 2.3 `cargo check -p truvis-render-interface` 验证编译通过

## 3. 创建 truvis-world crate

- [x] 3.1 创建 `engine/crates/truvis-world/` 目录结构，包含 `Cargo.toml` 和 `src/lib.rs`
- [x] 3.2 `Cargo.toml` 声明依赖 `truvis-scene` 和 `truvis-asset`
- [x] 3.3 `src/lib.rs` 定义 `World` 结构体（字段：scene_manager, asset_hub）
- [x] 3.4 在 workspace `Cargo.toml` 中注册 `truvis-world` 到 members 和 `[workspace.dependencies]`
- [x] 3.5 `cargo check -p truvis-world` 验证编译通过

## 4. Renderer 重构：替换 RenderContext

- [x] 4.1 在 `truvis-renderer` 的 `Cargo.toml` 中添加 `truvis-world` 依赖，移除不再直接需要的 `truvis-scene` 和 `truvis-asset`（如果它们只通过 World 访问的话；实际检查后决定）
- [x] 4.2 修改 `Renderer` 结构体：删除 `render_context: RenderContext` 字段，改为 `pub world: World` + `pub render_world: RenderWorld`
- [x] 4.3 更新 `Renderer::new()`：构造 World 和 RenderWorld 实例替代 RenderContext
- [x] 4.4 更新 `Renderer::begin_frame()`：将 `self.render_context.xxx` 引用改为 `self.render_world.xxx` 或 `self.world.xxx`
- [x] 4.5 更新 `Renderer::update_assets()`：通过 `self.world.asset_hub` 和 `self.render_world.gfx_resource_manager` / `self.render_world.bindless_manager` 访问
- [x] 4.6 更新 `Renderer::update_gpu_scene()`（`before_render` 内部）：从 `self.world.scene_manager.prepare_render_data()` extract，然后 `self.render_world.gpu_scene.upload_render_data()` upload
- [x] 4.7 更新 `Renderer::update_perframe_descriptor_set()`：访问 `self.render_world` 的字段
- [x] 4.8 更新 `Renderer` 其余方法（`end_frame`, `update_frame_settings`, `resize_frame_buffer`, `destroy` 等）
- [x] 4.9 删除 `render_context.rs` 文件（RenderContext 和 RenderContext2）
- [x] 4.10 `cargo check -p truvis-renderer` 验证编译通过

## 5. 更新 AppPlugin Contexts

- [x] 5.1 在 `truvis-app-api` 的 `Cargo.toml` 中添加 `truvis-world` 依赖
- [x] 5.2 修改 `InitCtx`：删除单独的 scene_manager / asset_hub / bindless_manager / gfx_resource_manager / global_descriptor_sets 字段，改为 `world: &mut World` + `render_world: &mut RenderWorld` + 保留 camera / swapchain_image_info / render_present / cmd_allocator
- [x] 5.3 修改 `UpdateCtx`：删除 `scene_manager` 字段，改为 `world: &mut World`；保留 pipeline_settings / frame_settings / delta_time_s
- [x] 5.4 修改 `RenderCtx`：删除 `render_context: &RenderContext` 字段，改为 `render_world: &RenderWorld`；保留 render_present / gui_draw_data / timeline
- [x] 5.5 修改 `ResizeCtx`：删除单独的 frame_settings / global_descriptor_sets / gfx_resource_manager / bindless_manager 字段，改为 `render_world: &mut RenderWorld`；保留 render_present
- [x] 5.6 更新 `truvis-app-api` 的 import 路径，移除对 `truvis_renderer::render_context::RenderContext` 的引用
- [x] 5.7 `cargo check -p truvis-app-api` 验证编译通过

## 6. 更新 FrameRuntime

- [x] 6.1 在 `truvis-frame-runtime` 的 `Cargo.toml` 中添加 `truvis-world` 依赖（如需直接引用 World 类型）
- [x] 6.2 更新 `FrameRuntime::init_after_window()`：构造新的 `InitCtx`，使用 `self.renderer.world` 和 `self.renderer.render_world`
- [x] 6.3 更新 `FrameRuntime::phase_update()`：构造新的 `UpdateCtx`，使用 `self.renderer.world` + 从 render_world 借出 pipeline_settings 和 frame_settings
- [x] 6.4 更新 `FrameRuntime::phase_render()`：构造新的 `RenderCtx`，使用 `&self.renderer.render_world`
- [x] 6.5 更新 `FrameRuntime::recreate_swapchain_if_needed()`：构造新的 `ResizeCtx`，使用 `&mut self.renderer.render_world`
- [x] 6.6 更新 `FrameRuntime::build_ui()` 和其他内部方法中对 `self.renderer.render_context` 的引用
- [x] 6.7 `cargo check -p truvis-frame-runtime` 验证编译通过

## 7. 迁移 Render Passes

- [x] 7.1 更新 `truvis-render-passes` 的 `Cargo.toml`：添加 `truvis-render-interface` 依赖（如未有），移除 `truvis-renderer` 依赖
- [x] 7.2 更新 `RealtimeRtRgPass` 和 `RealtimeRtPass`：`render_context: &RenderContext` → `render_world: &RenderWorld`，更新所有字段访问
- [x] 7.3 更新 `SdrRgPass` 和 `SdrPass`：同上
- [x] 7.4 更新 `BlitRgPass` 和 `BlitPass`：同上
- [x] 7.5 更新 `AccumRgPass` 和 `AccumPass`：同上
- [x] 7.6 更新 `DenoiseAccumRgPass` 和 `DenoiseAccumPass`：同上
- [x] 7.7 更新 `ResolveRgPass` 和 `ResolvePass`：同上
- [x] 7.8 更新 `PhongPass`：同上
- [x] 7.9 `cargo check -p truvis-render-passes` 验证编译通过（此时应不再依赖 truvis-renderer）

## 8. 更新 truvis-app 和 Demo Apps

- [x] 8.1 更新 `GuiRgPass`（`truvis-app/src/gui_rg_pass.rs`）：`render_context` → `render_world`
- [x] 8.2 更新 `RtPipeline`（`truvis-app/src/render_pipeline/rt_render_graph.rs`）：所有 `render_context` 参数和字段访问改为 `render_world`
- [x] 8.3 更新 `ShaderToyPass` 和 `ShaderToyApp`：同上
- [x] 8.4 更新 `TriangleApp`：适配新的 `InitCtx`、`UpdateCtx`、`RenderCtx`
- [x] 8.5 更新 `CornellApp`：同上
- [x] 8.6 更新 `SponzaApp`：同上
- [x] 8.7 `cargo check -p truvis-app` 验证编译通过

## 9. 整体验证与文档

- [x] 9.1 `cargo check --workspace` 验证全量编译通过
- [x] 9.2 更新 `ARCHITECTURE.md`：分层图中加入 `truvis-world`（L3），更新主干依赖链，更新核心模块职责描述
- [x] 9.3 更新 `truvis-render-interface` 的 crate 描述/README（如有），说明新增 RenderWorld
- [x] 9.4 清理：确认无残留的 `RenderContext` / `RenderContext2` 引用（`render_context` 变量名可保留为 `render_world`）
