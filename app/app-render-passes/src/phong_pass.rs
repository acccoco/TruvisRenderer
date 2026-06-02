use std::{mem::offset_of, rc::Rc};

use ash::vk;

use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_gfx::resources::layout::GfxVertexLayout;
use truvis_gfx::resources::vertex_layout::soa_3d::VertexLayoutSoA3D;
use truvis_gfx::{
    basic::color::LabelColor,
    commands::command_buffer::GfxCommandBuffer,
    pipelines::{
        graphics_pipeline::{GfxGraphicsPipeline, GfxGraphicsPipelineCreateInfo, GfxPipelineLayout},
        rendering_info::GfxRenderingInfo,
    },
};
use truvis_path::TruvisPath;
use truvis_render_foundation::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_foundation::gpu_store::GpuStore;
use truvis_render_foundation::pipeline_settings::FrameLabel;
use truvis_render_foundation::render_scene_view::RenderSceneView;
use truvis_shader_binding::gpu;

pub struct PhongPass {
    pipeline: GfxGraphicsPipeline,
}

/// Phong pass 本帧绘制目标，由调用方显式提供。
///
/// 目标资源属于 app 层具体管线；pass 只消费已经解析好的 Vulkan image view，
/// 不从 `GpuStore` 中假定某个全局 render target owner。
#[derive(Clone, Copy)]
pub struct PhongPassTarget {
    /// 本帧 color attachment 的 raw Vulkan view；owner 负责保证 view 在 draw 期间有效。
    pub color_view: vk::ImageView,
    /// 本帧 depth attachment 的 raw Vulkan view；PhongPass 不假定 depth 由哪个 pipeline owner 持有。
    pub depth_view: vk::ImageView,
    /// attachment 对应的绘制范围，用于 dynamic rendering extent、viewport 和 scissor。
    pub extent: vk::Extent2D,
}

impl PhongPass {
    pub fn new(
        ctx: GfxDeviceCtx<'_>,
        color_format: vk::Format,
        depth_format: vk::Format,
        render_descriptor_sets: &GlobalDescriptorSets,
    ) -> Self {
        let mut ci = GfxGraphicsPipelineCreateInfo::default();
        ci.vertex_shader_stage(&TruvisPath::shader_build_path_str("phong/phong3d.vs.slang"), c"main");
        ci.fragment_shader_stage(&TruvisPath::shader_build_path_str("phong/phong.ps.slang"), c"main");

        ci.vertex_binding(VertexLayoutSoA3D::vertex_input_bindings());
        ci.vertex_attribute(VertexLayoutSoA3D::vertex_input_attributes());

        ci.attach_info(vec![color_format], Some(depth_format), None);
        ci.color_blend(
            vec![
                vk::PipelineColorBlendAttachmentState::default()
                    .blend_enable(false)
                    .color_write_mask(vk::ColorComponentFlags::RGBA),
            ],
            [0.0; 4],
        );

        let pipeline_layout = Rc::new(GfxPipelineLayout::new(
            ctx,
            &render_descriptor_sets.global_set_layouts(),
            &[vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
                .offset(0)
                .size(size_of::<gpu::raster::PushConstants>() as u32)],
            "phong-pass",
        ));

        let d3_pipe = GfxGraphicsPipeline::new(ctx, &ci, pipeline_layout, "phong-d3-pipe");

        Self { pipeline: d3_pipe }
    }

    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        self.pipeline.destroy(ctx);
    }

    fn bind(
        &self,
        cmd: &GfxCommandBuffer,
        gpu_store: &GpuStore,
        viewport: &vk::Rect2D,
        push_constant: &gpu::raster::PushConstants,
        frame_label: FrameLabel,
    ) {
        cmd.cmd_bind_pipeline(vk::PipelineBindPoint::GRAPHICS, self.pipeline.handle());
        cmd.cmd_set_viewport(
            0,
            &[vk::Viewport {
                x: viewport.offset.x as f32,
                y: viewport.offset.y as f32 + viewport.extent.height as f32,
                width: viewport.extent.width as f32,
                height: -(viewport.extent.height as f32),
                min_depth: 0.0,
                max_depth: 1.0,
            }],
        );
        cmd.cmd_set_scissor(0, &[*viewport]);
        cmd.cmd_push_constants(
            self.pipeline.layout(),
            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            0,
            BytesConvert::bytes_of(push_constant),
        );

        let render_descriptor_sets = &gpu_store.global_descriptor_sets;
        cmd.bind_descriptor_sets(
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline.layout(),
            0,
            &render_descriptor_sets.global_sets(frame_label),
            None,
        );
    }

    pub fn draw(
        &self,
        cmd: &GfxCommandBuffer,
        gpu_store: &GpuStore,
        render_scene: &dyn RenderSceneView,
        target: PhongPassTarget,
    ) {
        let frame_label = gpu_store.frame_counter.frame_label();

        // target 由调用方显式传入，使 raster pass 与具体 main-view target owner 解耦。
        // 这样同一个 pass 后续可以绘制到不同 app-owned target，而不需要读全局 `GpuStore` 字段。
        let rendering_info = GfxRenderingInfo::new(
            vec![target.color_view],
            Some(target.depth_view),
            vk::Rect2D {
                offset: vk::Offset2D::default(),
                extent: target.extent,
            },
        );

        cmd.cmd_begin_rendering2(&rendering_info);
        cmd.begin_label("[phong-pass]draw", LabelColor::COLOR_PASS);

        self.bind(
            cmd,
            gpu_store,
            &target.extent.into(),
            &gpu::raster::PushConstants {
                frame_data: gpu_store.per_frame_data_buffers[*frame_label].device_address(),
                scene: render_scene.scene_buffer_device_address(frame_label),

                submesh_idx: 0,
                instance_idx: 0,

                _padding_1: Default::default(),
                _padding_2: Default::default(),
            },
            frame_label,
        );
        render_scene.draw_raster(frame_label, cmd, &mut |ins_idx, submesh_idx| {
            let data = [ins_idx, submesh_idx];
            cmd.cmd_push_constants(
                self.pipeline.layout(),
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                offset_of!(gpu::raster::PushConstants, instance_idx) as u32,
                bytemuck::bytes_of(&data),
            );
        });

        cmd.end_label();
        cmd.end_rendering();
    }
}
