use std::rc::Rc;
use std::sync::LazyLock;

use ash::vk;
use enum_map::{Enum, EnumMap, enum_map};
use itertools::Itertools;

use truvis_app_shader_binding::gpu;
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
            path: TruvisPath::shader_build_path_str("app", "post/resolve.slang"),
        },
        ShaderStage::Fragment => GfxShaderStageInfo {
            stage: vk::ShaderStageFlags::FRAGMENT,
            entry_point: c"psmain",
            path: TruvisPath::shader_build_path_str("app", "post/resolve.slang"),
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
    const DEBUG_IMAGE_MARGIN_PX: u32 = 16;
    const DEBUG_IMAGE_MAX_WIDTH_PX: u32 = 320;
    const DEBUG_IMAGE_MAX_HEIGHT_PX: u32 = 240;

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
            .size(size_of::<gpu::app::render_passes::resolve::PushConstant>() as u32);

        let descriptor_set_layout = GfxDescriptorSetLayout::<ResolveDescriptorBinding>::new(
            ctx,
            vk::DescriptorSetLayoutCreateFlags::PUSH_DESCRIPTOR_KHR,
            "resolve-pass-local-descriptor-layout",
        );
        let mut descriptor_set_layouts = global_descriptor_sets.global_set_layouts();
        assert_eq!(gpu::app::render_passes::resolve::SET_NUM, descriptor_set_layouts.len() as u32);
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

    /// 在一次 dynamic rendering 中绘制主图和可选 debug image。
    ///
    /// color attachment 只在这里 begin/end 一次，因此 `GfxRenderingInfo` 的 `CLEAR` 只作用于
    /// 主图绘制之前。debug image 通过第二次 descriptor/push constant 更新叠加，不能再次开启
    /// 一个同样使用 `CLEAR` 的 resolve scope，否则会清除已经写入的主画面。
    pub fn draw(
        &self,
        cmd: &GfxCommandBuffer,
        record_ctx: &RenderPassRecordCtx<'_>,
        color_attachment: vk::ImageView,
        target_extent: vk::Extent2D,
        main_image: &ResolvePassData,
        debug_image: Option<&ResolvePassData>,
    ) {
        let frame_label = record_ctx.frame_timing.frame_label();

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
            &record_ctx.shader_bindings.global_sets(frame_label),
            None,
        );

        self.draw_image(cmd, record_ctx, target_extent, main_image);
        if let Some(debug_image) = debug_image {
            self.draw_image(cmd, record_ctx, target_extent, debug_image);
        }

        cmd.end_rendering();
    }

    /// 更新当前 layer 的 sampled image 与像素矩形，并绘制一个无 vertex buffer quad。
    fn draw_image(
        &self,
        cmd: &GfxCommandBuffer,
        record_ctx: &RenderPassRecordCtx<'_>,
        target_extent: vk::Extent2D,
        params: &ResolvePassData,
    ) {
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
        cmd.push_descriptor_set(
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline_layout.handle(),
            gpu::app::render_passes::resolve::SET_NUM,
            &descriptor_writes,
        );

        let push_constant = gpu::app::render_passes::resolve::PushConstant {
            offset: params.offset.into(),
            size: params.size.into(),
            target_size: glam::vec2(target_extent.width as f32, target_extent.height as f32).into(),
            _padding_0: glam::Vec2::ZERO.into(),
        };
        cmd.cmd_push_constants(
            self.pipeline_layout.handle(),
            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            0,
            BytesConvert::bytes_of(&push_constant),
        );
        cmd.cmd_draw(6, 1, 0, 0);
    }

    /// 计算右侧垂直居中的 debug image 矩形。
    ///
    /// 预览不会放大超过源尺寸，并在 16px margin 内缩放到 320x240 的最大包围盒。
    /// 极小窗口或零尺寸源图直接返回 `None`，避免产生越界 viewport 坐标。
    fn debug_image_rect(source_extent: vk::Extent2D, target_extent: vk::Extent2D) -> Option<(glam::Vec2, glam::Vec2)> {
        if source_extent.width == 0 || source_extent.height == 0 {
            return None;
        }

        let double_margin = Self::DEBUG_IMAGE_MARGIN_PX * 2;
        let available_width = target_extent.width.saturating_sub(double_margin);
        let available_height = target_extent.height.saturating_sub(double_margin);
        let max_width = Self::DEBUG_IMAGE_MAX_WIDTH_PX.min(available_width);
        let max_height = Self::DEBUG_IMAGE_MAX_HEIGHT_PX.min(available_height);
        if max_width == 0 || max_height == 0 {
            return None;
        }

        let source_width = source_extent.width as f32;
        let source_height = source_extent.height as f32;
        let scale = (max_width as f32 / source_width).min(max_height as f32 / source_height).min(1.0);
        let width = (source_width * scale).floor().max(1.0);
        let height = (source_height * scale).floor().max(1.0);
        let offset = glam::vec2(
            target_extent.width as f32 - Self::DEBUG_IMAGE_MARGIN_PX as f32 - width,
            (target_extent.height as f32 - height) * 0.5,
        );
        Some((offset, glam::vec2(width, height)))
    }
}

/// Resolve pass 中可选的 debug image graph 输入。
///
/// `image` 只在当前 RenderGraph 内有效；`source_extent` 用于计算保持宽高比的右侧缩略图，
/// 不表达或拥有底层 GPU resource 生命周期。
#[derive(Clone, Copy)]
pub struct ResolveDebugImage {
    pub image: RgImageHandle,
    pub source_extent: vk::Extent2D,
}

pub struct ResolveRgPass<'a> {
    pub resolve_pass: &'a ResolvePass,

    pub record_ctx: RenderPassRecordCtx<'a>,

    pub render_target: RgImageHandle,
    pub debug_image: Option<ResolveDebugImage>,
    pub swapchain_image: RgImageHandle,

    pub swapchain_extent: vk::Extent2D,
}

impl RgPass for ResolveRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        // 声明写入 render target
        builder.read_image(self.render_target, RgImageState::SHADER_READ_FRAGMENT);
        if let Some(debug_image) = self.debug_image.filter(|debug_image| debug_image.image != self.render_target) {
            builder.read_image(debug_image.image, RgImageState::SHADER_READ_FRAGMENT);
        }
        builder.write_image(self.swapchain_image, RgImageState::COLOR_ATTACHMENT_READ_WRITE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        let cmd = ctx.cmd;

        let swapchain_image_view = ctx.get_image_view(self.swapchain_image).expect("ResolvePass: src_image not found");
        let render_target_view_handle =
            ctx.get_image_view_handle(self.render_target).expect("ResolvePass: render_target not found");
        let debug_image = self.debug_image.and_then(|debug_image| {
            let debug_image_view_handle =
                ctx.get_image_view_handle(debug_image.image).expect("ResolvePass: debug image not found");
            let (offset, size) = ResolvePass::debug_image_rect(debug_image.source_extent, self.swapchain_extent)?;
            Some(ResolvePassData {
                render_target: debug_image_view_handle,
                offset,
                size,
            })
        });

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
            debug_image.as_ref(),
        );
    }
}
