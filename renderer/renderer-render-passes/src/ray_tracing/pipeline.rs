use ash::vk;
use ash::vk::Handle;

use truvis_gfx::gfx::GfxDeviceCtx;

/// Realtime/offline ray tracing pass 共享的 Vulkan pipeline 及其显式释放契约。
pub struct GfxRtPipeline {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
}

impl Drop for GfxRtPipeline {
    fn drop(&mut self) {
        debug_assert!(self.pipeline.is_null(), "GfxRtPipeline pipeline dropped without explicit destroy");
        debug_assert!(self.pipeline_layout.is_null(), "GfxRtPipeline layout dropped without explicit destroy");
    }
}

impl GfxRtPipeline {
    pub fn destroy(mut self, ctx: GfxDeviceCtx<'_>) {
        if !self.pipeline.is_null() {
            unsafe {
                ctx.device().destroy_pipeline(self.pipeline, None);
            }
            self.pipeline = vk::Pipeline::null();
        }
        if !self.pipeline_layout.is_null() {
            unsafe {
                ctx.device().destroy_pipeline_layout(self.pipeline_layout, None);
            }
            self.pipeline_layout = vk::PipelineLayout::null();
        }
    }
}
