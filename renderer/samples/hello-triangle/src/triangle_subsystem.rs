use ash::vk;

use renderer_kit::subsystem::SubsystemLifecycle;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle, RgImageState};
use truvis_render_runtime::render_runtime::{RenderRuntimeInitCtx, RenderRuntimeShutdownCtx};

use crate::triangle_pass::TrianglePass;

#[derive(Default)]
pub struct TriangleSubsystem {
    triangle_pass: Option<TrianglePass>,
}

impl SubsystemLifecycle for TriangleSubsystem {
    fn init(&mut self, ctx: &mut RenderRuntimeInitCtx<'_>) {
        self.triangle_pass = Some(TrianglePass::new(ctx.device_ctx, ctx.swapchain_image_info.image_format));
    }

    fn shutdown(&mut self, ctx: &mut RenderRuntimeShutdownCtx<'_>) {
        if let Some(pass) = self.triangle_pass.take() {
            pass.destroy(ctx.device_ctx);
        }
    }
}

impl TriangleSubsystem {
    pub fn contribute_passes<'a>(
        &'a self,
        graph: &mut RenderGraphBuilder<'a>,
        canvas_color: RgImageHandle,
        canvas_extent: vk::Extent2D,
    ) {
        graph.add_pass_lambda(
            "triangle",
            move |builder| {
                builder.read_write_image(canvas_color, RgImageState::COLOR_ATTACHMENT_READ_WRITE);
            },
            move |context| {
                let canvas_view = context.get_image_view(canvas_color).unwrap();
                self.triangle_pass.as_ref().expect("TriangleSubsystem not initialized").draw(
                    context.cmd,
                    canvas_view,
                    canvas_extent,
                );
            },
        );
    }
}
