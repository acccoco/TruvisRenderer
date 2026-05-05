## 1. 定义 Renderer Ctx 类型

- [x] 1.1 在 `truvis-renderer/src/renderer.rs` 中定义 `RendererUpdateCtx` struct（字段：`&mut World`, `&mut PipelineSettings`, `&FrameSettings`, `&AccumData`, `vk::Extent2D`, `Duration`/`f32` delta_time）
- [x] 1.2 定义 `RendererRenderCtx` struct（字段：`&RenderWorld`, `&RenderPresent`, `&GfxSemaphore`）
- [x] 1.3 定义 `RendererInitCtx` struct（字段：`&mut World`, `&mut RenderWorld`, `&mut CmdAllocator`, `GfxSwapchainImageInfo`, `&RenderPresent`）
- [x] 1.4 定义 `RendererResizeCtx` struct（字段：`&mut RenderWorld`, `&RenderPresent`）
- [x] 1.5 `cargo check -p truvis-renderer` 验证类型定义编译通过

## 2. 重构 Renderer 生命周期方法

- [x] 2.1 将现有 `begin_frame()` 改为自包含：合并 `update_assets()` 到 `begin_frame` 内部（或紧随其后的内部调用），移除 pub `update_assets`
- [x] 2.2 将 `update_frame_settings()` 和 `acquire_image()` 变为私有，内化到新的 `update_phase()` 方法中
- [x] 2.3 实现 `pub fn update_phase(&mut self) -> RendererUpdateCtx<'_>`：内部调 update_frame_settings + acquire_image，然后返回 Ctx
- [x] 2.4 实现 `pub fn submit_gui_data(&mut self, draw_data: &imgui::DrawData)`：调用 `render_present.gui_backend.prepare_render_data`
- [x] 2.5 合并 `update_accum_frames` + `before_render` 为 `pub fn prepare(&mut self, camera: &Camera)`
- [x] 2.6 实现 `pub fn render_phase(&self) -> RendererRenderCtx<'_>`（注意：`&self`）
- [x] 2.7 将 `present_image()` 重命名为 `pub fn present(&mut self)`
- [x] 2.8 保持 `pub fn end_frame(&mut self)` 不变
- [x] 2.9 实现 `pub fn handle_resize(&mut self, new_size: [u32; 2]) -> Option<RendererResizeCtx<'_>>`：合并 update_window_size + need_resize + recreate_swapchain 逻辑
- [x] 2.10 重构 `init_after_window` 使其返回 `RendererInitCtx`（或提供单独的 `init_ctx()` 方法）
- [x] 2.11 移除或私有化不再需要 pub 的方法：`update_assets`, `update_frame_settings`, `acquire_image`, `need_resize`, `recreate_swapchain`, `update_accum_frames`, `before_render`, `frame_label`, `swapchain_image_info`
- [x] 2.12 保留 `pub fn time_to_render(&self) -> bool` 作为帧间查询方法
- [x] 2.13 `cargo check -p truvis-renderer` 验证编译通过

## 3. 更新 AppPlugin Ctx 和 trait 签名

- [x] 3.1 更新 `truvis-app-api` 中的 `InitCtx` 定义：移除 camera 字段，使用 `RendererInitCtx` 的字段（或直接使用 `RendererInitCtx`），Plugin init 签名接收 camera 作为独立参数
- [x] 3.2 评估 `UpdateCtx` 是否可以直接变为 `RendererUpdateCtx` 的 type alias，或保留现有结构但标明来源
- [x] 3.3 `RenderCtx` 保留 gui_draw_data 字段（由 FrameRuntime 组合），更新 render_world/render_present/timeline 字段使其与 `RendererRenderCtx` 对应
- [x] 3.4 更新 `ResizeCtx` 为 `RendererResizeCtx` 的 type alias 或对应结构
- [x] 3.5 更新 `AppPlugin` trait 的 `init` 方法签名：`fn init(&mut self, ctx: &mut RendererInitCtx, camera: &mut Camera)`
- [x] 3.6 `cargo check -p truvis-app-api` 验证编译通过

## 4. 重构 FrameRuntime

- [x] 4.1 重写 `run_frame()`：按新的生命周期顺序调用 Renderer 方法
- [x] 4.2 重写 update phase：获取 `renderer.update_phase()` Ctx，在 block 内完成 build_ui + plugin.update
- [x] 4.3 在 update Ctx drop 后调用 `gui_host.compile_ui()` + `renderer.submit_gui_data()`
- [x] 4.4 重写 camera update：在 submit_gui_data 之后、prepare 之前从 Renderer Ctx 读取必要信息或保存为局部变量
- [x] 4.5 重写 render phase：获取 `renderer.render_phase()` Ctx（`&self`），组合 `gui_host.get_render_data()` 构造 `RenderCtx`
- [x] 4.6 重写 `recreate_swapchain_if_needed`：调用 `renderer.handle_resize()`，如返回 Some 则传给 plugin.on_resize
- [x] 4.7 重写 `init_after_window`：使用 Renderer 返回的 `RendererInitCtx`，单独传入 camera 给 plugin.init
- [x] 4.8 移除所有 `self.renderer.world` / `self.renderer.render_world` / `self.renderer.timer` / `self.renderer.render_present` / `self.renderer.fif_timeline_semaphore` 的直接字段访问
- [x] 4.9 `cargo check -p truvis-frame-runtime` 验证编译通过

## 5. 更新 Demo Apps

- [x] 5.1 更新 `TriangleApp`：适配新的 `init` 签名（camera 独立参数），其他 hooks 如有字段名变化则同步更新
- [x] 5.2 更新 `CornellApp`：同上
- [x] 5.3 更新 `SponzaApp`：同上
- [x] 5.4 更新 `ShaderToyApp`：同上
- [x] 5.5 `cargo check -p truvis-app` 验证编译通过

## 6. 整体验证与清理

- [x] 6.1 `cargo check --workspace` 全量编译通过
- [x] 6.2 确认 Renderer 中无 AppPlugin / FrameRuntime / overlay 相关的 import 或类型引用
- [x] 6.3 确认 FrameRuntime 中无 `self.renderer.world` / `self.renderer.render_world` 等直接字段路径
- [x] 6.4 更新 `ARCHITECTURE.md`：补充三层生命周期模型的描述，更新运行时序图
- [ ] 6.5 运行一个 demo（如 triangle）确认无功能回归
