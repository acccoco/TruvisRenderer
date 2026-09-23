use std::mem::{offset_of, size_of};
use std::rc::Rc;

use ash::vk;

use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_gfx::pipelines::graphics_pipeline::{GfxGraphicsPipeline, GfxGraphicsPipelineCreateInfo, GfxPipelineLayout};
use truvis_gfx::resources::layout::GfxVertexLayout;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_shader_manifest::ShaderArtifactPath;

pub use truvis_renderer_shader_binding::gpu::renderer::render_passes::viewport_overlay::Vertex as ViewportOverlayVertex;

/// Vertex input 的格式和 offset 从 canonical Renderer ABI 生成类型取得。
pub struct ViewportOverlayVertexLayout;

impl GfxVertexLayout for ViewportOverlayVertexLayout {
    fn vertex_input_bindings() -> Vec<vk::VertexInputBindingDescription> {
        vec![
            vk::VertexInputBindingDescription::default()
                .binding(0)
                .stride(size_of::<ViewportOverlayVertex>() as u32)
                .input_rate(vk::VertexInputRate::VERTEX),
        ]
    }

    fn vertex_input_attributes() -> Vec<vk::VertexInputAttributeDescription> {
        vec![
            vk::VertexInputAttributeDescription::default()
                .binding(0)
                .location(0)
                .format(vk::Format::R32G32B32A32_SFLOAT)
                .offset(offset_of!(ViewportOverlayVertex, position) as u32),
            vk::VertexInputAttributeDescription::default()
                .binding(0)
                .location(1)
                .format(vk::Format::R32G32B32A32_SFLOAT)
                .offset(offset_of!(ViewportOverlayVertex, color) as u32),
        ]
    }

    fn buffer_size(vertex_cnt: usize) -> usize {
        vertex_cnt * size_of::<ViewportOverlayVertex>()
    }
}

/// 只录制最终画面的无深度 overlay；selection、投影和 buffer 生命周期属于上层 subsystem。
pub struct ViewportOverlayPass {
    pipeline: GfxGraphicsPipeline,
}

impl ViewportOverlayPass {
    pub fn new(ctx: GfxDeviceCtx<'_>, format: vk::Format) -> Self {
        let mut ci = GfxGraphicsPipelineCreateInfo::default();
        let shader = ShaderArtifactPath::resolve("renderer", "ui/viewport_overlay.slang");
        ci.vertex_shader_stage(&shader, c"vsmain");
        ci.fragment_shader_stage(&shader, c"psmain");
        ci.vertex_binding(ViewportOverlayVertexLayout::vertex_input_bindings());
        ci.vertex_attribute(ViewportOverlayVertexLayout::vertex_input_attributes());
        ci.attach_info(vec![format], None, None);
        ci.cull_mode(vk::CullModeFlags::NONE, vk::FrontFace::COUNTER_CLOCKWISE);
        ci.depth_test(None, false, false);
        ci.color_blend(
            vec![
                vk::PipelineColorBlendAttachmentState::default()
                    .blend_enable(true)
                    .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                    .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                    .color_blend_op(vk::BlendOp::ADD)
                    .src_alpha_blend_factor(vk::BlendFactor::ONE)
                    .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                    .alpha_blend_op(vk::BlendOp::ADD)
                    .color_write_mask(vk::ColorComponentFlags::RGBA),
            ],
            [0.0; 4],
        );
        let layout = Rc::new(GfxPipelineLayout::new(ctx, &[], &[], "viewport-overlay"));
        Self {
            pipeline: GfxGraphicsPipeline::new(ctx, &ci, layout, "viewport-overlay"),
        }
    }

    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        self.pipeline.destroy(ctx);
    }
}

pub struct ViewportOverlayRgPass<'a> {
    pub pass: &'a ViewportOverlayPass,
    pub present_image: RgImageHandle,
    pub extent: vk::Extent2D,
    pub vertices: vk::Buffer,
    pub vertex_count: u32,
}

impl RgPass for ViewportOverlayRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        builder.read_write_image(self.present_image, RgImageState::COLOR_ATTACHMENT_READ_WRITE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        let attachment = vk::RenderingAttachmentInfo::default()
            .image_view(ctx.get_image_view(self.present_image).expect("viewport overlay present target").handle())
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::STORE);
        let rect = vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent: self.extent,
        };
        let info = vk::RenderingInfo::default()
            .layer_count(1)
            .render_area(rect)
            .color_attachments(std::slice::from_ref(&attachment));
        ctx.cmd.cmd_begin_rendering(&info);
        ctx.cmd.cmd_bind_pipeline(vk::PipelineBindPoint::GRAPHICS, self.pass.pipeline.handle());
        ctx.cmd.cmd_set_viewport(
            0,
            &[vk::Viewport {
                x: 0.0,
                y: self.extent.height as f32,
                width: self.extent.width as f32,
                height: -(self.extent.height as f32),
                min_depth: 0.0,
                max_depth: 1.0,
            }],
        );
        ctx.cmd.cmd_set_scissor(0, &[rect]);
        ctx.cmd.cmd_bind_vertex_buffers(0, &[self.vertices], &[0]);
        ctx.cmd.cmd_draw(self.vertex_count, 1, 0, 0);
        ctx.cmd.end_rendering();
    }
}
