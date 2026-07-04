use std::rc::Rc;

use ash::vk;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_gfx::pipelines::graphics_pipeline::{GfxGraphicsPipeline, GfxGraphicsPipelineCreateInfo, GfxPipelineLayout};
use truvis_path::TruvisPath;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::bindings::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;

/// 主视图右下角的相机朝向坐标轴 gizmo pass。
///
/// 该 pass 不持有几何 buffer，也不接触 CPU scene。三条坐标轴由 shader 通过 `SV_VertexID`
/// 生成，并从全局 set 2 的 `per_frame_data.view` 读取当前相机旋转。调用方只需要把它插入
/// present graph，并保证目标 image 已经是当前帧要叠加的 present image。
pub struct CoordinateGizmoPass {
    pipeline: GfxGraphicsPipeline,
}

/// coordinate gizmo 的绘制目标。
///
/// `present_view` 由 RenderGraph 导入的当前 present image view 提供。pass 使用 `LOAD`
/// 保留已有主视图、selection outline 和其它 overlay 内容，只在右下角 scissor 内叠加三轴。
#[derive(Clone, Copy)]
pub struct CoordinateGizmoTarget {
    pub present_view: vk::ImageView,
    pub extent: vk::Extent2D,
}

impl CoordinateGizmoPass {
    const GIZMO_SIZE_PX: u32 = 112;
    const GIZMO_MARGIN_PX: u32 = 24;
    const AXIS_VERTEX_COUNT: u32 = 27;

    pub fn new(
        ctx: GfxDeviceCtx<'_>,
        present_format: vk::Format,
        global_descriptor_sets: &GlobalDescriptorSets,
    ) -> Self {
        let mut ci = GfxGraphicsPipelineCreateInfo::default();
        ci.vertex_shader_stage(&TruvisPath::shader_build_path_str("ui/coordinate_gizmo.slang"), c"vsmain");
        ci.fragment_shader_stage(&TruvisPath::shader_build_path_str("ui/coordinate_gizmo.slang"), c"psmain");
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

        let pipeline_layout =
            Rc::new(GfxPipelineLayout::new(ctx, &global_descriptor_sets.global_set_layouts(), &[], "coordinate-gizmo"));
        let pipeline = GfxGraphicsPipeline::new(ctx, &ci, pipeline_layout, "coordinate-gizmo");

        Self { pipeline }
    }

    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        self.pipeline.destroy(ctx);
    }
}

impl CoordinateGizmoPass {
    pub fn draw(&self, cmd: &GfxCommandBuffer, record_ctx: &RenderPassRecordCtx<'_>, target: CoordinateGizmoTarget) {
        let Some(gizmo_rect) = Self::gizmo_rect(target.extent) else {
            return;
        };

        let color_attachment = vk::RenderingAttachmentInfo::default()
            .image_view(target.present_view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::STORE);
        let render_info = vk::RenderingInfo::default()
            .layer_count(1)
            .render_area(gizmo_rect)
            .color_attachments(std::slice::from_ref(&color_attachment));

        let frame_label = record_ctx.frame_timing.frame_label();
        cmd.cmd_begin_rendering(&render_info);
        cmd.cmd_bind_pipeline(vk::PipelineBindPoint::GRAPHICS, self.pipeline.handle());
        Self::set_gizmo_viewport(cmd, gizmo_rect);
        cmd.bind_descriptor_sets(
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline.layout(),
            0,
            &record_ctx.shader_bindings.global_sets(frame_label),
            None,
        );
        cmd.cmd_draw(Self::AXIS_VERTEX_COUNT, 1, 0, 0);
        cmd.end_rendering();
    }

    fn gizmo_rect(extent: vk::Extent2D) -> Option<vk::Rect2D> {
        let min_side = extent.width.min(extent.height);
        if min_side == 0 {
            return None;
        }

        let size = Self::GIZMO_SIZE_PX.min(min_side);
        let margin = Self::GIZMO_MARGIN_PX.min(size / 4);
        let x = extent.width.saturating_sub(size + margin);
        let y = extent.height.saturating_sub(size + margin);
        Some(vk::Rect2D {
            offset: vk::Offset2D {
                x: x as i32,
                y: y as i32,
            },
            extent: vk::Extent2D {
                width: size,
                height: size,
            },
        })
    }

    fn set_gizmo_viewport(cmd: &GfxCommandBuffer, rect: vk::Rect2D) {
        // 使用负 viewport height 维持项目现有 Vulkan clip-space 到屏幕空间的 Y 轴约定。
        // rect.offset 是右下角小视口的左上角，viewport.y 需要放到该区域底边。
        cmd.cmd_set_viewport(
            0,
            &[vk::Viewport {
                x: rect.offset.x as f32,
                y: rect.offset.y as f32 + rect.extent.height as f32,
                width: rect.extent.width as f32,
                height: -(rect.extent.height as f32),
                min_depth: 0.0,
                max_depth: 1.0,
            }],
        );
        cmd.cmd_set_scissor(0, &[rect]);
    }
}

/// coordinate gizmo 的 RenderGraph adapter。
///
/// pass 只读全局 per-frame descriptor，并以 color attachment `LOAD` 方式叠加 present image。
/// 因此 RenderGraph 中只需要声明 present image 的读写状态。
pub struct CoordinateGizmoRgPass<'a> {
    pub gizmo_pass: &'a CoordinateGizmoPass,
    pub record_ctx: RenderPassRecordCtx<'a>,
    pub present_image: RgImageHandle,
    pub extent: vk::Extent2D,
}

impl RgPass for CoordinateGizmoRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        builder.read_write_image(self.present_image, RgImageState::COLOR_ATTACHMENT_READ_WRITE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        let present_view = ctx.get_image_view(self.present_image).expect("CoordinateGizmo: present image not found");
        self.gizmo_pass.draw(
            ctx.cmd,
            &self.record_ctx,
            CoordinateGizmoTarget {
                present_view: present_view.handle(),
                extent: self.extent,
            },
        );
    }
}
