use std::rc::Rc;

use ash::vk;
use itertools::Itertools;

use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::pipelines::graphics_pipeline::{GfxGraphicsPipeline, GfxGraphicsPipelineCreateInfo, GfxPipelineLayout};
use truvis_gfx::pipelines::rendering_info::GfxRenderingInfo;
use truvis_gfx::pipelines::shader::GfxShaderStageInfo;
use truvis_path::TruvisPath;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_interface::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_interface::handles::GfxImageViewHandle;
use truvis_renderer::render_context::RenderContext;
use truvis_shader_binding::gpu;
use truvis_utils::count_indexed_array;
use truvis_utils::enumed_map;

enumed_map!(ShaderStage<GfxShaderStageInfo>: {
    Vertex: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::VERTEX,
        entry_point: c"vsmain",
        path: TruvisPath::shader_build_path_str("resolve/resolve.slang"),
    },
    Fragment: GfxShaderStageInfo {
        stage: vk::ShaderStageFlags::FRAGMENT,
        entry_point: c"psmain",
        path: TruvisPath::shader_build_path_str("resolve/resolve.slang"),
    },
});

/// 用于绘制的参数
pub struct ResolvePassData {
    /// 源图像的 texture handle（将从 bindless_textures 中采样）
    pub render_target: GfxImageViewHandle,
    /// 采样器类型
    pub sampler_type: gpu::ESamplerType,
    /// 在 color attachment 上的偏移量（像素坐标）
    pub offset: glam::Vec2,
    /// 绘制区域的大小（像素尺寸）
    pub size: glam::Vec2,
}

/// Resolve Pass
///
/// 功能：将指定的 image 按照给定的 offset 和 size 绘制到 color attachment
///
/// - 使用固定的边长为1的正方形作为顶点（无需顶点缓冲区，顶点数据在着色器中内置）
/// - 通过 bindless descriptor 指定需要绘制的 image
/// - 使用 push constant 传递 offset、size 等参数
pub struct ResolvePass {
    pipeline: GfxGraphicsPipeline,
    pipeline_layout: Rc<GfxPipelineLayout>,
}

impl ResolvePass {
    /// # 参数
    /// - `color_format`: color attachment 的格式
    /// - `render_descriptor_sets`: 全局描述符集
    pub fn new(global_descriptor_sets: &GlobalDescriptorSets, color_format: vk::Format) -> Self {
        let mut pipeline_ci = GfxGraphicsPipelineCreateInfo::default();

        // 着色器阶段
        pipeline_ci.shader_stages(ShaderStage::iter().map(|stage| stage.value().clone()).collect_vec());

        // Attachment 配置：只有 color，没有 depth
        pipeline_ci.attach_info(vec![color_format], None, Some(vk::Format::UNDEFINED));

        // 不需要顶点输入，顶点数据在着色器中内置
        pipeline_ci.vertex_binding(vec![]);
        pipeline_ci.vertex_attribute(vec![]);

        // Color blending：启用 alpha 混合（src_alpha, one_minus_src_alpha）
        pipeline_ci.color_blend(
            vec![
                vk::PipelineColorBlendAttachmentState::default()
                    .blend_enable(true)
                    .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                    .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                    .color_blend_op(vk::BlendOp::ADD)
                    .src_alpha_blend_factor(vk::BlendFactor::ONE)
                    .dst_alpha_blend_factor(vk::BlendFactor::ZERO)
                    .alpha_blend_op(vk::BlendOp::ADD)
                    .color_write_mask(vk::ColorComponentFlags::RGBA),
            ],
            [0.0; 4],
        );

        // Pipeline layout：包含全局描述符集和 push constant
        let push_constant_range = vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .offset(0)
            .size(size_of::<gpu::resolve::PushConstant>() as u32);

        let pipeline_layout = Rc::new(GfxPipelineLayout::new(
            &global_descriptor_sets.global_set_layouts(),
            &[push_constant_range],
            "resolve-pass",
        ));

        let pipeline = GfxGraphicsPipeline::new(&pipeline_ci, pipeline_layout.clone(), "resolve-pipeline");

        Self {
            pipeline,
            pipeline_layout,
        }
    }

    /// 绘制指定的 image 到 color attachment
    ///
    /// # 参数
    /// - `cmd`: 命令缓冲区
    /// - `render_context`: 渲染上下文
    /// - `frame_label`: 当前帧标签
    /// - `color_attachment`: 目标 color attachment 的 image view
    /// - `target_extent`: 目标区域的尺寸
    /// - `params`: 绘制参数（源图像、偏移、大小等）
    pub fn draw(
        &self,
        cmd: &GfxCommandBuffer,
        render_context: &RenderContext,
        color_attachment: vk::ImageView,
        target_extent: vk::Extent2D,
        params: &ResolvePassData,
    ) {
        let frame_label = render_context.frame_counter.frame_label();

        // 获取源图像的 bindless handle
        let src_srv_handle = render_context.bindless_manager.get_shader_srv_handle(params.render_target);

        // 构造 push constant
        let push_constant = gpu::resolve::PushConstant {
            src_texture: src_srv_handle.0,
            sampler_type: params.sampler_type,
            offset: params.offset.into(),
            size: params.size.into(),
            target_size: glam::vec2(target_extent.width as f32, target_extent.height as f32).into(),
        };

        // 设置渲染区域
        let rendering_info = GfxRenderingInfo::new(
            vec![color_attachment],
            None,
            vk::Rect2D {
                offset: vk::Offset2D::default(),
                extent: target_extent,
            },
        );

        // 开始渲染
        cmd.cmd_begin_rendering2(&rendering_info);
        cmd.cmd_bind_pipeline(vk::PipelineBindPoint::GRAPHICS, self.pipeline.handle());

        // 设置 viewport（Y 轴翻转以适配 Vulkan 坐标系）
        cmd.cmd_set_viewport(
            0,
            &[vk::Viewport {
                x: 0.0,
                y: target_extent.height as f32,
                width: target_extent.width as f32,
                height: -(target_extent.height as f32),
                min_depth: 0.0,
                max_depth: 1.0,
            }],
        );

        cmd.cmd_set_scissor(
            0,
            &[vk::Rect2D {
                offset: vk::Offset2D::default(),
                extent: target_extent,
            }],
        );

        // 绑定描述符集
        cmd.bind_descriptor_sets(
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline_layout.handle(),
            0,
            &render_context.global_descriptor_sets.global_sets(frame_label),
            None,
        );

        // Push constants
        cmd.cmd_push_constants(
            self.pipeline_layout.handle(),
            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            0,
            BytesConvert::bytes_of(&push_constant),
        );

        // 绘制 6 个顶点（两个三角形组成的矩形）
        cmd.cmd_draw(6, 1, 0, 0);

        cmd.end_rendering();
    }
}

pub struct ResolveRgPass<'a> {
    pub resolve_pass: &'a ResolvePass,

    // TODO 暂时使用这个肮脏的实现
    pub render_context: &'a RenderContext,

    pub render_target: RgImageHandle,
    pub swapchain_image: RgImageHandle,

    pub swapchain_extent: vk::Extent2D,
}

impl RgPass for ResolveRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        // 声明写入 render target
        builder.read_image(self.render_target, RgImageState::SHADER_READ_FRAGMENT);
        builder.write_image(self.swapchain_image, RgImageState::COLOR_ATTACHMENT_READ_WRITE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        let cmd = ctx.cmd;

        let swapchain_image_view = ctx.get_image_view(self.swapchain_image).expect("ResolvePass: src_image not found");
        let render_target_view_handle =
            ctx.get_image_view_handle(self.render_target).expect("ResolvePass: render_target not found");

        self.resolve_pass.draw(
            cmd,
            self.render_context,
            swapchain_image_view.handle(),
            self.swapchain_extent,
            &ResolvePassData {
                render_target: render_target_view_handle,
                sampler_type: gpu::ESamplerType_LinearClamp,
                offset: glam::vec2(0.0, 0.0),
                size: glam::vec2(self.swapchain_extent.width as f32, self.swapchain_extent.height as f32),
            },
        );
    }
}
