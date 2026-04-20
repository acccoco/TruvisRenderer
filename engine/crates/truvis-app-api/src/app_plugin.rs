//! Plugin contract and phased contexts.

use truvis_asset::asset_hub::AssetHub;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::swapchain::swapchain::GfxSwapchainImageInfo;
use truvis_render_interface::bindless_manager::BindlessManager;
use truvis_render_interface::cmd_allocator::CmdAllocator;
use truvis_render_interface::gfx_resource_manager::GfxResourceManager;
use truvis_render_interface::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_interface::pipeline_settings::{FrameSettings, PipelineSettings};
use truvis_renderer::platform::camera::Camera;
use truvis_renderer::present::render_present::RenderPresent;
use truvis_renderer::render_context::RenderContext;
use truvis_scene::scene_manager::SceneManager;

// ---------------------------------------------------------------------------
// Phase Contexts
// ---------------------------------------------------------------------------

/// Init phase context — one-time setup after window/surface creation.
pub struct InitCtx<'a> {
    pub camera: &'a mut Camera,
    pub swapchain_image_info: GfxSwapchainImageInfo,
    pub global_descriptor_sets: &'a GlobalDescriptorSets,
    pub render_present: &'a RenderPresent,
    pub cmd_allocator: &'a mut CmdAllocator,
    pub scene_manager: &'a mut SceneManager,
    pub asset_hub: &'a mut AssetHub,
    pub gfx_resource_manager: &'a mut GfxResourceManager,
    pub bindless_manager: &'a mut BindlessManager,
}

/// Per-frame CPU update context.
pub struct UpdateCtx<'a> {
    pub scene_manager: &'a mut SceneManager,
    pub pipeline_settings: &'a mut PipelineSettings,
    pub frame_settings: &'a FrameSettings,
    pub delta_time_s: f32,
}

/// Render phase context — GPU command recording & submission.
pub struct RenderCtx<'a> {
    pub render_context: &'a RenderContext,
    pub render_present: &'a RenderPresent,
    pub gui_draw_data: &'a imgui::DrawData,
    pub timeline: &'a GfxSemaphore,
}

/// Swapchain resize context.
pub struct ResizeCtx<'a> {
    pub frame_settings: &'a FrameSettings,
    pub render_present: &'a RenderPresent,
    pub global_descriptor_sets: &'a GlobalDescriptorSets,
    pub gfx_resource_manager: &'a mut GfxResourceManager,
    pub bindless_manager: &'a mut BindlessManager,
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
