//! Plugin contract and phased contexts.

use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::swapchain::swapchain::GfxSwapchainImageInfo;
use truvis_render_interface::cmd_allocator::CmdAllocator;
use truvis_render_interface::pipeline_settings::{FrameSettings, PipelineSettings};
use truvis_render_interface::render_world::RenderWorld;
use truvis_renderer::platform::camera::Camera;
use truvis_renderer::present::render_present::RenderPresent;
use truvis_world::World;

// ---------------------------------------------------------------------------
// Phase Contexts
// ---------------------------------------------------------------------------

/// Init phase context — one-time setup after window/surface creation.
pub struct InitCtx<'a> {
    pub camera: &'a mut Camera,
    pub swapchain_image_info: GfxSwapchainImageInfo,
    pub render_present: &'a RenderPresent,
    pub cmd_allocator: &'a mut CmdAllocator,
    pub world: &'a mut World,
    pub render_world: &'a mut RenderWorld,
}

/// Per-frame CPU update context.
pub struct UpdateCtx<'a> {
    pub world: &'a mut World,
    pub pipeline_settings: &'a mut PipelineSettings,
    pub frame_settings: &'a FrameSettings,
    pub delta_time_s: f32,
}

/// Render phase context — GPU command recording & submission.
pub struct RenderCtx<'a> {
    pub render_world: &'a RenderWorld,
    pub render_present: &'a RenderPresent,
    pub gui_draw_data: &'a imgui::DrawData,
    pub timeline: &'a GfxSemaphore,
}

/// Swapchain resize context.
pub struct ResizeCtx<'a> {
    pub render_world: &'a mut RenderWorld,
    pub render_present: &'a RenderPresent,
}

// ---------------------------------------------------------------------------
// AppPlugin
// ---------------------------------------------------------------------------

/// Application plugin — `FrameRuntime` phase hook contract.
///
/// Hook order: `init` (once) → per frame: `build_ui` → `update` → `render`.
/// `on_resize` fires after swapchain rebuild. `shutdown` before destruction.
/// `prepare` is runtime-internal — not a plugin hook.
pub trait AppPlugin {
    fn init(&mut self, ctx: &mut InitCtx);
    fn update(&mut self, ctx: &mut UpdateCtx);
    fn build_ui(&mut self, ui: &imgui::Ui);
    fn render(&self, ctx: &RenderCtx);
    fn on_resize(&mut self, _ctx: &mut ResizeCtx) {}
    fn shutdown(&mut self) {}
}
