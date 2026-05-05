//! Unified Plugin trait and plugin-facing phase contexts.

use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::swapchain::swapchain::GfxSwapchainImageInfo;
use truvis_render_interface::cmd_allocator::CmdAllocator;
use truvis_render_interface::pipeline_settings::{FrameSettings, PipelineSettings};
use truvis_render_interface::render_world::RenderWorld;
use truvis_renderer::present::render_present::RenderPresent;
use truvis_world::World;

use crate::input_event::InputEvent;

/// One-time initialization context for app-owned plugins.
pub struct PluginInitCtx<'a> {
    pub world: &'a mut World,
    pub render_world: &'a mut RenderWorld,
    pub cmd_allocator: &'a mut CmdAllocator,
    pub swapchain_image_info: GfxSwapchainImageInfo,
    pub render_present: &'a RenderPresent,
}

/// CPU update context for app-owned plugins.
pub struct PluginUpdateCtx<'a> {
    pub world: &'a mut World,
    pub pipeline_settings: &'a mut PipelineSettings,
    pub frame_settings: &'a FrameSettings,
    pub delta_time_s: f32,
}

/// Render context for app-owned plugins.
///
/// GUI draw data intentionally stays inside the concrete GUI plugin.
pub struct PluginRenderCtx<'a> {
    pub render_world: &'a RenderWorld,
    pub render_present: &'a RenderPresent,
    pub timeline: &'a GfxSemaphore,
}

/// Resize context for plugins with swapchain-sized resources.
pub struct PluginResizeCtx<'a> {
    pub render_world: &'a mut RenderWorld,
    pub render_present: &'a RenderPresent,
}

/// Standard lifecycle for reusable app-owned capability units.
///
/// Specific capabilities such as `ui()` or `contribute_passes()` stay on the
/// concrete plugin type so the app can compose plugins without downcasting.
pub trait Plugin {
    fn init(&mut self, _ctx: &mut PluginInitCtx) {}

    fn on_input(&mut self, _event: &InputEvent) -> bool {
        false
    }

    fn update(&mut self, _ctx: &mut PluginUpdateCtx) {}

    fn on_resize(&mut self, _ctx: &mut PluginResizeCtx) {}

    fn shutdown(&mut self) {}
}
