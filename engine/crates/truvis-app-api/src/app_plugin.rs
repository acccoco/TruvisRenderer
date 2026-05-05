//! Plugin contract and phased contexts.

use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_render_interface::pipeline_settings::{FrameSettings, PipelineSettings};
use truvis_render_interface::render_world::RenderWorld;
use truvis_renderer::platform::camera::Camera;
use truvis_renderer::present::render_present::RenderPresent;
use truvis_renderer::renderer::{RendererInitCtx, RendererResizeCtx};
use truvis_world::World;

// ---------------------------------------------------------------------------
// Phase Contexts
// ---------------------------------------------------------------------------

/// Init phase context — directly uses Renderer's `RendererInitCtx`.
pub type InitCtx<'a> = RendererInitCtx<'a>;

/// Per-frame CPU update context.
///
/// Subset of `RendererUpdateCtx` fields relevant to plugins.
pub struct UpdateCtx<'a> {
    pub world: &'a mut World,
    pub pipeline_settings: &'a mut PipelineSettings,
    pub frame_settings: &'a FrameSettings,
    pub delta_time_s: f32,
}

/// Render phase context — GPU command recording & submission.
///
/// Composed by FrameRuntime from `RendererRenderCtx` + gui_draw_data from GuiHost.
pub struct RenderCtx<'a> {
    pub render_world: &'a RenderWorld,
    pub render_present: &'a RenderPresent,
    pub gui_draw_data: &'a imgui::DrawData,
    pub timeline: &'a GfxSemaphore,
}

/// Swapchain resize context — directly uses Renderer's `RendererResizeCtx`.
pub type ResizeCtx<'a> = RendererResizeCtx<'a>;

// ---------------------------------------------------------------------------
// AppPlugin
// ---------------------------------------------------------------------------

/// Application plugin — `FrameRuntime` phase hook contract.
///
/// Hook order: `init` (once) → per frame: `build_ui` → `update` → `render`.
/// `on_resize` fires after swapchain rebuild. `shutdown` before destruction.
/// `prepare` is runtime-internal — not a plugin hook.
pub trait AppPlugin {
    fn init(&mut self, ctx: &mut InitCtx, camera: &mut Camera);
    fn update(&mut self, ctx: &mut UpdateCtx);
    fn build_ui(&mut self, ui: &imgui::Ui);
    fn render(&self, ctx: &RenderCtx);
    fn on_resize(&mut self, _ctx: &mut ResizeCtx) {}
    fn shutdown(&mut self) {}
}
