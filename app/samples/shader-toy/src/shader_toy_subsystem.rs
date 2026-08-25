use ash::vk;

use app_kit::subsystem::{SubsystemLifecycle, SubsystemRenderCtx};
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle, RgImageState};
use truvis_render_runtime::render_runtime::{RenderRuntimeInitCtx, RenderRuntimeShutdownCtx};

use crate::shader_toy_pass::ShaderToyPass;

#[derive(Default)]
pub struct ShaderToySubsystem {
    shader_toy_pass: Option<ShaderToyPass>,
}

impl SubsystemLifecycle for ShaderToySubsystem {
    fn init(&mut self, ctx: &mut RenderRuntimeInitCtx<'_>) {
        self.shader_toy_pass = Some(ShaderToyPass::new(ctx.device_ctx, ctx.swapchain_image_info.image_format));
    }

    fn shutdown(&mut self, ctx: &mut RenderRuntimeShutdownCtx<'_>) {
        if let Some(pass) = self.shader_toy_pass.take() {
            pass.destroy(ctx.device_ctx);
        }
    }
}

impl ShaderToySubsystem {
    pub fn contribute_passes<'a>(
        &'a self,
        graph: &mut RenderGraphBuilder<'a>,
        ctx: &'a SubsystemRenderCtx<'a>,
        canvas_color: RgImageHandle,
        canvas_extent: vk::Extent2D,
    ) {
        graph.add_pass_lambda(
            "shader-toy",
            move |builder| {
                builder.read_write_image(canvas_color, RgImageState::COLOR_ATTACHMENT_READ_WRITE);
            },
            move |context| {
                let canvas_view = context.get_image_view(canvas_color).unwrap();
                self.shader_toy_pass.as_ref().expect("ShaderToySubsystem not initialized").draw(
                    &ctx.record_ctx,
                    context.cmd,
                    canvas_view,
                    canvas_extent,
                );
            },
        );
    }
}
