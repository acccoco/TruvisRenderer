use std::rc::Rc;
use std::sync::LazyLock;

use ash::vk;
use enum_map::{Enum, EnumMap, enum_map};
use itertools::Itertools;

use truvis_descriptor_layout_macro::DescriptorBinding;
use truvis_gfx::basic::bytes::BytesConvert;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::descriptors::descriptor::GfxDescriptorSetLayout;
use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_gfx::pipelines::graphics_pipeline::{GfxGraphicsPipeline, GfxGraphicsPipelineCreateInfo, GfxPipelineLayout};
use truvis_gfx::pipelines::rendering_info::GfxRenderingInfo;
use truvis_gfx::pipelines::shader::GfxShaderStageInfo;
use truvis_gfx::utilities::descriptor_cursor::GfxDescriptorCursor;
use truvis_path::TruvisPath;
use truvis_render_foundation::handles::GfxImageViewHandle;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::bindings::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;
use truvis_shader_binding::gpu;

#[derive(Debug, Clone, Copy, Enum)]
enum ShaderStage {
    Vertex,
    Fragment,
}

static SHADER_STAGES: LazyLock<EnumMap<ShaderStage, GfxShaderStageInfo>> = LazyLock::new(|| {
    enum_map! {
        ShaderStage::Vertex => GfxShaderStageInfo {
            stage: vk::ShaderStageFlags::VERTEX,
            entry_point: c"vsmain",
            path: TruvisPath::shader_build_path_str("post/resolve.slang"),
        },
        ShaderStage::Fragment => GfxShaderStageInfo {
            stage: vk::ShaderStageFlags::FRAGMENT,
            entry_point: c"psmain",
            path: TruvisPath::shader_build_path_str("post/resolve.slang"),
        },
    }
});

/// 用于绘制的参数
pub struct ResolvePassData {
    /// 源图像 view，由当前 draw 的 pass-local sampled-image descriptor 引用。
    pub render_target: GfxImageViewHandle,
    /// 在 color attachment 上的偏移量（像素坐标）
    pub offset: glam::Vec2,
    /// 绘制区域的大小（像素尺寸）
    pub size: glam::Vec2,
}

/// Resolve Pass 实现
///
/// 功能：将指定的 image 按照给定的 offset 和 size 绘制到 color attachment
///
/// - 使用固定的边长为1的正方形作为顶点（无需顶点缓冲区，顶点数据在着色器中内置）
/// - 通过 pass-local descriptor 指定需要绘制的 image
/// - 使用 push constant 传递 offset、size 等参数
#[derive(DescriptorBinding)]
struct ResolveDescriptorBinding {
    #[binding = 0]
    #[descriptor_type = "SAMPLED_IMAGE"]
    #[stage = "FRAGMENT"]
    #[count = 1]
    _src_texture: (),
}

pub struct ResolvePass {
    pipeline: GfxGraphicsPipeline,
    pipeline_layout: Rc<GfxPipelineLayout>,
    descriptor_set_layout: GfxDescriptorSetLayout<ResolveDescriptorBinding>,
}

impl ResolvePass {
    /// # 参数
    /// - `color_format`: color attachment 的格式
    /// - `render_descriptor_sets`: 全局描述符集
    pub fn new(ctx: GfxDeviceCtx<'_>, global_descriptor_sets: &GlobalDescriptorSets, color_format: vk::Format) -> Self {
        let mut pipeline_ci = GfxGraphicsPipelineCreateInfo::default();

        // 着色器阶段
        pipeline_ci.shader_stages(SHADER_STAGES.values().cloned().collect_vec());

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

        // Pipeline layout：全局 set 之后追加当前 draw 的 sampled-image push descriptor set。
        let push_constant_range = vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .offset(0)
            .size(size_of::<gpu::resolve::PushConstant>() as u32);

        let descriptor_set_layout = GfxDescriptorSetLayout::<ResolveDescriptorBinding>::new(
            ctx,
            vk::DescriptorSetLayoutCreateFlags::PUSH_DESCRIPTOR_KHR,
            "resolve-pass-local-descriptor-layout",
        );
        let mut descriptor_set_layouts = global_descriptor_sets.global_set_layouts();
        assert_eq!(gpu::RESOLVE_SET_NUM, descriptor_set_layouts.len() as u32);
        descriptor_set_layouts.push(descriptor_set_layout.handle());
        let pipeline_layout =
            Rc::new(GfxPipelineLayout::new(ctx, &descriptor_set_layouts, &[push_constant_range], "resolve-pass"));

        let pipeline = GfxGraphicsPipeline::new(ctx, &pipeline_ci, pipeline_layout.clone(), "resolve-pipeline");

        Self {
            pipeline,
            pipeline_layout,
            descriptor_set_layout,
        }
    }

    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        self.pipeline.destroy(ctx);
        self.pipeline_layout.destroy(ctx);
        self.descriptor_set_layout.destroy(ctx);
    }

    /// 绘制指定的 image 到 color attachment
    ///
    /// # 参数
    /// - `cmd`: 命令缓冲区
    /// - `record_ctx`: pass 录制上下文
    /// - `frame_label`: 当前帧标签
    /// - `color_attachment`: 目标 color attachment 的 image view
    /// - `target_extent`: 目标区域的尺寸
    /// - `params`: 绘制参数（源图像、偏移、大小等）
    pub fn draw(
        &self,
        cmd: &GfxCommandBuffer,
        record_ctx: &RenderPassRecordCtx<'_>,
        color_attachment: vk::ImageView,
        target_extent: vk::Extent2D,
        params: &ResolvePassData,
    ) {
        let frame_label = record_ctx.frame_timing.frame_label();

        let src_view = record_ctx
            .gfx_resource_manager
            .get_image_view(params.render_target)
            .expect("ResolvePass: source image view not found")
            .handle();
        let descriptor_writes = [ResolveDescriptorBinding::src_texture().write_image(
            vk::DescriptorSet::null(),
            0,
            vec![
                vk::DescriptorImageInfo::default()
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .image_view(src_view),
            ],
        )];

        // 构造 push constant
        let push_constant = gpu::resolve::PushConstant {
            offset: params.offset.into(),
            size: params.size.into(),
            target_size: glam::vec2(target_extent.width as f32, target_extent.height as f32).into(),
            _padding_0: glam::Vec2::ZERO.into(),
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
        cmd.push_descriptor_set(
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline_layout.handle(),
            gpu::RESOLVE_SET_NUM,
            &descriptor_writes,
        );

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
            &record_ctx.shader_bindings.global_sets(frame_label),
            None,
        );

        // 写入 push constants
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

    pub record_ctx: RenderPassRecordCtx<'a>,

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
            &self.record_ctx,
            swapchain_image_view.handle(),
            self.swapchain_extent,
            &ResolvePassData {
                render_target: render_target_view_handle,
                offset: glam::vec2(0.0, 0.0),
                size: glam::vec2(self.swapchain_extent.width as f32, self.swapchain_extent.height as f32),
            },
        );
    }
}
