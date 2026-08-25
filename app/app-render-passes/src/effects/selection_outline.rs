use std::mem::{offset_of, size_of};
use std::rc::Rc;

use ash::vk;

use truvis_app_shader_binding::gpu;
use truvis_descriptor_layout_macro::DescriptorBinding;
use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::descriptors::descriptor::GfxDescriptorSetLayout;
use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_gfx::pipelines::graphics_pipeline::{GfxGraphicsPipeline, GfxGraphicsPipelineCreateInfo, GfxPipelineLayout};
use truvis_gfx::utilities::descriptor_cursor::GfxDescriptorCursor;
use truvis_path::TruvisPath;
use truvis_render_foundation::handles::GfxImageViewHandle;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::bindings::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;
use truvis_render_runtime::selection::{WorldSubmeshRasterView, WorldSubmeshSelection};

/// selection outline 的共享 pass 实现。
///
/// 该 pass 拥有两条 graphics pipeline：mask pipeline 把选中 submesh 光栅化到 R8 mask；
/// composite pipeline 再用全屏 quad 采样 mask 邻域并叠加到 present image。它不拥有
/// mask image，也不注册 debug image；窗口尺寸 mask 资源由具体 App 持有。
#[derive(DescriptorBinding)]
struct SelectionOutlineCompositeDescriptorBinding {
    #[binding = 0]
    #[descriptor_type = "SAMPLED_IMAGE"]
    #[stage = "FRAGMENT"]
    #[count = 1]
    _mask_texture: (),
}

pub struct SelectionOutlinePass {
    mask_pipeline: GfxGraphicsPipeline,
    composite_pipeline: GfxGraphicsPipeline,
    composite_descriptor_set_layout: GfxDescriptorSetLayout<SelectionOutlineCompositeDescriptorBinding>,
}

/// mask pass 的目标。
///
/// mask image 必须是 `R8_UNORM`、`COLOR_ATTACHMENT | SAMPLED`，并在同一 frame label 下
/// 保持有效。pass 每帧会清零整张 mask，因此不依赖上一帧内容。
#[derive(Clone, Copy)]
pub struct SelectionOutlineMaskTarget {
    pub view: vk::ImageView,
    pub extent: vk::Extent2D,
}

/// composite pass 的目标。
///
/// `present_view` 由 RenderGraph 导入的当前 swapchain image view 提供；draw 使用
/// `LOAD` 保留 resolve 已写入的主视图，再输出 alpha blended outline。
#[derive(Clone, Copy)]
pub struct SelectionOutlineCompositeTarget {
    pub mask_view_handle: GfxImageViewHandle,
    pub present_view: vk::ImageView,
    pub extent: vk::Extent2D,
}

impl SelectionOutlinePass {
    pub const MASK_FORMAT: vk::Format = vk::Format::R8_UNORM;
    const OUTLINE_RADIUS_PX: f32 = 2.0;
    const OUTLINE_COLOR: [f32; 4] = [0.08, 0.92, 1.0, 0.88];

    pub fn new(
        ctx: GfxDeviceCtx<'_>,
        present_format: vk::Format,
        global_descriptor_sets: &GlobalDescriptorSets,
    ) -> Self {
        let mask_pipeline = Self::create_mask_pipeline(ctx, global_descriptor_sets);
        let composite_descriptor_set_layout = GfxDescriptorSetLayout::<SelectionOutlineCompositeDescriptorBinding>::new(
            ctx,
            vk::DescriptorSetLayoutCreateFlags::PUSH_DESCRIPTOR_KHR,
            "selection-outline-composite-local-descriptor-layout",
        );
        let composite_pipeline = Self::create_composite_pipeline(
            ctx,
            present_format,
            global_descriptor_sets,
            &composite_descriptor_set_layout,
        );
        Self {
            mask_pipeline,
            composite_pipeline,
            composite_descriptor_set_layout,
        }
    }

    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        self.mask_pipeline.destroy(ctx);
        self.composite_pipeline.destroy(ctx);
        self.composite_descriptor_set_layout.destroy(ctx);
    }

    fn create_mask_pipeline(
        ctx: GfxDeviceCtx<'_>,
        global_descriptor_sets: &GlobalDescriptorSets,
    ) -> GfxGraphicsPipeline {
        let mut ci = GfxGraphicsPipelineCreateInfo::default();
        ci.vertex_shader_stage(&TruvisPath::shader_build_path_str("app", "selection_outline/mask.vs.slang"), c"main");
        ci.fragment_shader_stage(&TruvisPath::shader_build_path_str("app", "selection_outline/mask.ps.slang"), c"main");
        ci.vertex_binding(vec![vk::VertexInputBindingDescription {
            binding: 0,
            stride: size_of::<glam::Vec3>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }]);
        ci.vertex_attribute(vec![vk::VertexInputAttributeDescription {
            binding: 0,
            location: 0,
            format: vk::Format::R32G32B32_SFLOAT,
            offset: 0,
        }]);
        ci.attach_info(vec![Self::MASK_FORMAT], None, None);
        ci.cull_mode(vk::CullModeFlags::NONE, vk::FrontFace::COUNTER_CLOCKWISE);
        ci.depth_test(None, false, false);
        ci.color_blend(
            vec![
                vk::PipelineColorBlendAttachmentState::default()
                    .blend_enable(false)
                    .color_write_mask(vk::ColorComponentFlags::R),
            ],
            [0.0; 4],
        );

        let pipeline_layout = Rc::new(GfxPipelineLayout::new(
            ctx,
            &global_descriptor_sets.global_set_layouts(),
            &[vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX)
                .offset(0)
                .size(size_of::<gpu::app::render_passes::selection_outline::MaskPushConstant>() as u32)],
            "selection-outline-mask",
        ));

        GfxGraphicsPipeline::new(ctx, &ci, pipeline_layout, "selection-outline-mask")
    }

    fn create_composite_pipeline(
        ctx: GfxDeviceCtx<'_>,
        present_format: vk::Format,
        global_descriptor_sets: &GlobalDescriptorSets,
        descriptor_set_layout: &GfxDescriptorSetLayout<SelectionOutlineCompositeDescriptorBinding>,
    ) -> GfxGraphicsPipeline {
        let mut ci = GfxGraphicsPipelineCreateInfo::default();
        ci.vertex_shader_stage(
            &TruvisPath::shader_build_path_str("app", "selection_outline/composite.slang"),
            c"vsmain",
        );
        ci.fragment_shader_stage(
            &TruvisPath::shader_build_path_str("app", "selection_outline/composite.slang"),
            c"psmain",
        );
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

        let mut descriptor_set_layouts = global_descriptor_sets.global_set_layouts();
        assert_eq!(gpu::app::render_passes::selection_outline::SET_NUM, descriptor_set_layouts.len() as u32);
        descriptor_set_layouts.push(descriptor_set_layout.handle());
        let pipeline_layout = Rc::new(GfxPipelineLayout::new(
            ctx,
            &descriptor_set_layouts,
            &[vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
                .offset(0)
                .size(size_of::<gpu::app::render_passes::selection_outline::CompositePushConstant>() as u32)],
            "selection-outline-composite",
        ));

        GfxGraphicsPipeline::new(ctx, &ci, pipeline_layout, "selection-outline-composite")
    }

    pub fn draw_mask(
        &self,
        cmd: &GfxCommandBuffer,
        record_ctx: &RenderPassRecordCtx<'_>,
        selected_raster: &dyn WorldSubmeshRasterView,
        selection: WorldSubmeshSelection,
        target: SelectionOutlineMaskTarget,
    ) {
        let clear_value = vk::ClearValue {
            color: vk::ClearColorValue { float32: [0.0; 4] },
        };
        let color_attachment = vk::RenderingAttachmentInfo::default()
            .image_view(target.view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(clear_value);
        let render_info = vk::RenderingInfo::default()
            .layer_count(1)
            .render_area(target.extent.into())
            .color_attachments(std::slice::from_ref(&color_attachment));

        let frame_label = record_ctx.frame_timing.frame_label();
        cmd.cmd_begin_rendering(&render_info);
        cmd.cmd_bind_pipeline(vk::PipelineBindPoint::GRAPHICS, self.mask_pipeline.handle());
        Self::set_fullscreen_viewport(cmd, target.extent);

        let push_constants = gpu::app::render_passes::selection_outline::MaskPushConstant {
            instance_idx: 0,
            submesh_idx: 0,
            _padding_0: 0,
            _padding_1: 0,
        };
        cmd.cmd_push_constants(
            self.mask_pipeline.layout(),
            vk::ShaderStageFlags::VERTEX,
            0,
            BytesConvert::bytes_of(&push_constants),
        );
        cmd.bind_descriptor_sets(
            vk::PipelineBindPoint::GRAPHICS,
            self.mask_pipeline.layout(),
            0,
            &record_ctx.shader_bindings.global_sets(frame_label),
            None,
        );

        selected_raster.draw_selected_submesh_raster(frame_label, cmd, selection, &mut |instance_idx, submesh_idx| {
            let data = [instance_idx, submesh_idx];
            cmd.cmd_push_constants(
                self.mask_pipeline.layout(),
                vk::ShaderStageFlags::VERTEX,
                offset_of!(gpu::app::render_passes::selection_outline::MaskPushConstant, instance_idx) as u32,
                BytesConvert::bytes_of(&data),
            );
        });

        cmd.end_rendering();
    }

    pub fn draw_composite(
        &self,
        cmd: &GfxCommandBuffer,
        record_ctx: &RenderPassRecordCtx<'_>,
        target: SelectionOutlineCompositeTarget,
    ) {
        let frame_label = record_ctx.frame_timing.frame_label();
        let mask_view = record_ctx
            .gfx_resource_manager
            .get_image_view(target.mask_view_handle)
            .expect("SelectionOutlinePass: mask image view not found")
            .handle();
        let descriptor_writes = [SelectionOutlineCompositeDescriptorBinding::mask_texture().write_image(
            vk::DescriptorSet::null(),
            0,
            vec![
                vk::DescriptorImageInfo::default()
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .image_view(mask_view),
            ],
        )];
        let push_constants = gpu::app::render_passes::selection_outline::CompositePushConstant {
            target_size: glam::vec2(target.extent.width as f32, target.extent.height as f32).into(),
            _padding_0: glam::Vec2::ZERO.into(),
            color: glam::Vec4::from_array(Self::OUTLINE_COLOR).into(),
            radius_px: Self::OUTLINE_RADIUS_PX,
            _padding_1: 0.0,
            _padding_2: 0.0,
            _padding_3: 0.0,
        };

        let color_attachment = vk::RenderingAttachmentInfo::default()
            .image_view(target.present_view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::STORE);
        let render_info = vk::RenderingInfo::default()
            .layer_count(1)
            .render_area(target.extent.into())
            .color_attachments(std::slice::from_ref(&color_attachment));

        cmd.cmd_begin_rendering(&render_info);
        cmd.cmd_bind_pipeline(vk::PipelineBindPoint::GRAPHICS, self.composite_pipeline.handle());
        cmd.push_descriptor_set(
            vk::PipelineBindPoint::GRAPHICS,
            self.composite_pipeline.layout(),
            gpu::app::render_passes::selection_outline::SET_NUM,
            &descriptor_writes,
        );
        Self::set_fullscreen_viewport(cmd, target.extent);
        cmd.bind_descriptor_sets(
            vk::PipelineBindPoint::GRAPHICS,
            self.composite_pipeline.layout(),
            0,
            &record_ctx.shader_bindings.global_sets(frame_label),
            None,
        );
        cmd.cmd_push_constants(
            self.composite_pipeline.layout(),
            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            0,
            BytesConvert::bytes_of(&push_constants),
        );
        cmd.cmd_draw(6, 1, 0, 0);
        cmd.end_rendering();
    }

    fn set_fullscreen_viewport(cmd: &GfxCommandBuffer, extent: vk::Extent2D) {
        cmd.cmd_set_viewport(
            0,
            &[vk::Viewport {
                x: 0.0,
                y: extent.height as f32,
                width: extent.width as f32,
                height: -(extent.height as f32),
                min_depth: 0.0,
                max_depth: 1.0,
            }],
        );
        cmd.cmd_set_scissor(
            0,
            &[vk::Rect2D {
                offset: vk::Offset2D::default(),
                extent,
            }],
        );
    }
}

/// selection outline mask 的 RenderGraph adapter。
///
/// mask 每帧完整 clear + draw，因此导入时可以使用 `UNDEFINED_TOP`，不需要保留旧内容。
pub struct SelectionOutlineMaskRgPass<'a> {
    pub outline_pass: &'a SelectionOutlinePass,
    pub record_ctx: RenderPassRecordCtx<'a>,
    pub selected_raster: &'a dyn WorldSubmeshRasterView,
    pub selection: WorldSubmeshSelection,
    pub mask_image: RgImageHandle,
    pub extent: vk::Extent2D,
}

impl RgPass for SelectionOutlineMaskRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        builder.write_image(self.mask_image, RgImageState::COLOR_ATTACHMENT_WRITE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        let mask_view = ctx.get_image_view(self.mask_image).expect("SelectionOutlineMask: mask image not found");
        self.outline_pass.draw_mask(
            ctx.cmd,
            &self.record_ctx,
            self.selected_raster,
            self.selection,
            SelectionOutlineMaskTarget {
                view: mask_view.handle(),
                extent: self.extent,
            },
        );
    }
}

/// selection outline composite 的 RenderGraph adapter。
///
/// composite 读取上一 pass 写出的 mask，并以 `LOAD` attachment 方式叠加到 present image。
pub struct SelectionOutlineCompositeRgPass<'a> {
    pub outline_pass: &'a SelectionOutlinePass,
    pub record_ctx: RenderPassRecordCtx<'a>,
    pub mask_image: RgImageHandle,
    pub present_image: RgImageHandle,
    pub extent: vk::Extent2D,
}

impl RgPass for SelectionOutlineCompositeRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        builder.read_image(self.mask_image, RgImageState::SHADER_READ_FRAGMENT);
        builder.read_write_image(self.present_image, RgImageState::COLOR_ATTACHMENT_READ_WRITE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        let mask_view_handle =
            ctx.get_image_view_handle(self.mask_image).expect("SelectionOutlineComposite: mask image not found");
        let present_view =
            ctx.get_image_view(self.present_image).expect("SelectionOutlineComposite: present not found");

        self.outline_pass.draw_composite(
            ctx.cmd,
            &self.record_ctx,
            SelectionOutlineCompositeTarget {
                mask_view_handle,
                present_view: present_view.handle(),
                extent: self.extent,
            },
        );
    }
}
