//! 统一的 Plugin trait 与面向 plugin 的阶段上下文。

use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::swapchain::swapchain::GfxSwapchainImageInfo;
use truvis_render_backend::present::render_present::RenderPresent;
use truvis_render_interface::cmd_allocator::CmdAllocator;
use truvis_render_interface::pipeline_settings::{FrameSettings, PipelineSettings};
use truvis_render_interface::render_world::RenderWorld;
use truvis_world::World;

use crate::input_event::InputEvent;

/// app-owned plugin 的一次性初始化上下文。
pub struct PluginInitCtx<'a> {
    pub world: &'a mut World,
    pub render_world: &'a mut RenderWorld,
    pub cmd_allocator: &'a mut CmdAllocator,
    pub swapchain_image_info: GfxSwapchainImageInfo,
    pub render_present: &'a RenderPresent,
}

/// app-owned plugin 的 CPU 更新上下文。
pub struct PluginUpdateCtx<'a> {
    pub world: &'a mut World,
    pub pipeline_settings: &'a mut PipelineSettings,
    pub frame_settings: &'a FrameSettings,
    pub delta_time_s: f32,
}

/// app-owned plugin 的渲染上下文。
///
/// GUI draw data 刻意保留在具体 GUI plugin 内部。
pub struct PluginRenderCtx<'a> {
    pub render_world: &'a RenderWorld,
    pub render_present: &'a RenderPresent,
    pub timeline: &'a GfxSemaphore,
}

/// 持有 swapchain 尺寸资源的 plugin 使用的 resize 上下文。
pub struct PluginResizeCtx<'a> {
    pub render_world: &'a mut RenderWorld,
    pub render_present: &'a RenderPresent,
}

/// 可复用 app-owned 能力单元的标准生命周期。
///
/// `ui()` 或 `contribute_passes()` 等特有能力保留在具体 plugin 类型上，
/// 这样 app 可以组合 plugin，而无需 downcast。
pub trait Plugin {
    fn init(&mut self, _ctx: &mut PluginInitCtx) {}

    fn on_input(&mut self, _event: &InputEvent) -> bool {
        false
    }

    fn update(&mut self, _ctx: &mut PluginUpdateCtx) {}

    fn on_resize(&mut self, _ctx: &mut PluginResizeCtx) {}

    fn shutdown(&mut self) {}
}
