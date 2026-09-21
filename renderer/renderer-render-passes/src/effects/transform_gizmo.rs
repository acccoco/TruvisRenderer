use std::mem::size_of;
use std::rc::Rc;

use ash::vk;

use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_gfx::pipelines::graphics_pipeline::{GfxGraphicsPipeline, GfxGraphicsPipelineCreateInfo, GfxPipelineLayout};
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::bindings::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;
use truvis_renderer_shader_binding::gpu;
use truvis_shader_manifest::ShaderArtifactPath;

pub const NO_AXIS: u32 = gpu::renderer::render_passes::transform_gizmo::NO_AXIS;

/// Transform gizmo 的本帧 world-space 绘制数据。
///
/// 该类型只表达 overlay 需要的数值，不携带 GameWorld、MeshInstance 或 RenderWorld
/// 身份；CPU hit test 与 GPU 绘制的目标由上层 Renderer 分别拥有。
#[derive(Clone, Copy)]
pub struct TransformGizmoDrawData {
    pub origin_ws: glam::Vec3,
    pub axes_ws: [glam::Vec3; 3],
    pub axis_length_ws: f32,
    pub hovered_axis: u32,
    pub active_axis: u32,
}

/// 选中对象 transform gizmo 的 graphics pass。
pub struct TransformGizmoPass {
    pipeline: GfxGraphicsPipeline,
}

/// transform gizmo 的 color overlay 目标。
#[derive(Clone, Copy)]
pub struct TransformGizmoTarget {
    pub present_view: vk::ImageView,
    pub extent: vk::Extent2D,
    pub draw_data: TransformGizmoDrawData,
}

impl TransformGizmoPass {
    const AXIS_VERTEX_COUNT: u32 = 27;

    pub fn new(
        ctx: GfxDeviceCtx<'_>,
        present_format: vk::Format,
        global_descriptor_sets: &GlobalDescriptorSets,
    ) -> Self {
        let mut ci = GfxGraphicsPipelineCreateInfo::default();
        ci.vertex_shader_stage(&ShaderArtifactPath::resolve("renderer", "ui/transform_gizmo.slang"), c"vsmain");
        ci.fragment_shader_stage(&ShaderArtifactPath::resolve("renderer", "ui/transform_gizmo.slang"), c"psmain");
        ci.vertex_binding(vec![]);
        ci.vertex_attribute(vec![]);
        ci.attach_info(vec![present_format], None, None);
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

        let pipeline_layout = Rc::new(GfxPipelineLayout::new(
            ctx,
            &global_descriptor_sets.global_set_layouts(),
            &[vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX)
                .offset(0)
                .size(size_of::<gpu::renderer::render_passes::transform_gizmo::PushConstant>() as u32)],
            "transform-gizmo",
        ));
        let pipeline = GfxGraphicsPipeline::new(ctx, &ci, pipeline_layout, "transform-gizmo");
        Self { pipeline }
    }

    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        self.pipeline.destroy(ctx);
    }

    pub fn draw(&self, cmd: &GfxCommandBuffer, record_ctx: &RenderPassRecordCtx<'_>, target: TransformGizmoTarget) {
        if target.extent.width == 0 || target.extent.height == 0 || !target.draw_data.axis_length_ws.is_finite() {
            return;
        }

        let color_attachment = vk::RenderingAttachmentInfo::default()
            .image_view(target.present_view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::STORE);
        let render_info = vk::RenderingInfo::default()
            .layer_count(1)
            .render_area(vk::Rect2D {
                offset: vk::Offset2D::default(),
                extent: target.extent,
            })
            .color_attachments(std::slice::from_ref(&color_attachment));

        let draw_data = target.draw_data;
        let push_constant = gpu::renderer::render_passes::transform_gizmo::PushConstant {
            origin_ws: draw_data.origin_ws.extend(1.0).into(),
            axis_x_ws: draw_data.axes_ws[0].extend(0.0).into(),
            axis_y_ws: draw_data.axes_ws[1].extend(0.0).into(),
            axis_z_ws: draw_data.axes_ws[2].extend(0.0).into(),
            axis_length_ws: draw_data.axis_length_ws,
            hovered_axis: draw_data.hovered_axis,
            active_axis: draw_data.active_axis,
            _padding_0: 0,
        };

        let frame_label = record_ctx.frame_timing.frame_label();
        cmd.cmd_begin_rendering(&render_info);
        cmd.cmd_bind_pipeline(vk::PipelineBindPoint::GRAPHICS, self.pipeline.handle());
        cmd.cmd_set_viewport(
            0,
            &[vk::Viewport {
                x: 0.0,
                y: target.extent.height as f32,
                width: target.extent.width as f32,
                height: -(target.extent.height as f32),
                min_depth: 0.0,
                max_depth: 1.0,
            }],
        );
        cmd.cmd_set_scissor(
            0,
            &[vk::Rect2D {
                offset: vk::Offset2D::default(),
                extent: target.extent,
            }],
        );
        cmd.bind_descriptor_sets(
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline.layout(),
            0,
            &record_ctx.shader_bindings.global_sets(frame_label),
            None,
        );
        cmd.cmd_push_constants(
            self.pipeline.layout(),
            vk::ShaderStageFlags::VERTEX,
            0,
            BytesConvert::bytes_of(&push_constant),
        );
        cmd.cmd_draw(Self::AXIS_VERTEX_COUNT, 1, 0, 0);
        cmd.end_rendering();
    }
}

/// transform gizmo 的 RenderGraph adapter。
pub struct TransformGizmoRgPass<'a> {
    pub gizmo_pass: &'a TransformGizmoPass,
    pub record_ctx: RenderPassRecordCtx<'a>,
    pub present_image: RgImageHandle,
    pub extent: vk::Extent2D,
    pub draw_data: TransformGizmoDrawData,
}

impl RgPass for TransformGizmoRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        builder.read_write_image(self.present_image, RgImageState::COLOR_ATTACHMENT_READ_WRITE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        let present_view = ctx.get_image_view(self.present_image).expect("TransformGizmo: present image not found");
        self.gizmo_pass.draw(
            ctx.cmd,
            &self.record_ctx,
            TransformGizmoTarget {
                present_view: present_view.handle(),
                extent: self.extent,
                draw_data: self.draw_data,
            },
        );
    }
}
