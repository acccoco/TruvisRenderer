use ash::vk;

use truvis_descriptor_layout_macro::DescriptorBinding;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_gfx::utilities::descriptor_cursor::GfxDescriptorCursor;
use truvis_render_foundation::handles::GfxImageViewHandle;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::bindings::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;
use truvis_renderer_shader_binding::gpu;
use truvis_shader_manifest::ShaderArtifactPath;

use crate::compute_pass::ComputePass;

/// 累积 Pass 的数据
pub struct AccumPassData {
    pub single_frame_image: GfxImageViewHandle,
    pub accum_image: GfxImageViewHandle,
    pub image_size: vk::Extent2D,
    /// 调用方维护的离线历史样本数。pass 只按这个数做 running average，
    /// 不能回读 runtime `ViewAccumState`，否则会把 realtime temporal reset 语义混入离线 reference。
    pub accum_frames: u32,
}

#[derive(DescriptorBinding)]
struct AccumDescriptorBinding {
    #[binding = 0]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "COMPUTE"]
    #[count = 1]
    _single_frame_input: (),

    #[binding = 1]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "COMPUTE"]
    #[count = 1]
    _accum_output: (),
}

/// 累积 Pass - 将单帧 RT 结果累积到 accum_image 中。
///
/// 两张 image 都由当前 dispatch 的 pass-local descriptor 绑定；历史 image 的所有权和有效性仍由
/// OfflineRenderSubsystem 管理，RenderGraph 负责同一 image 的跨 dispatch 读写同步。
pub struct AccumPass {
    accum_pass: ComputePass<gpu::renderer::render_passes::post_accum::PushConstant, AccumDescriptorBinding>,
}

impl AccumPass {
    pub fn new(ctx: GfxDeviceCtx<'_>, render_descriptor_sets: &GlobalDescriptorSets) -> Self {
        let accum_pass =
            ComputePass::<gpu::renderer::render_passes::post_accum::PushConstant, AccumDescriptorBinding>::new(
                ctx,
                render_descriptor_sets,
                gpu::renderer::render_passes::post_accum::SET_NUM,
                c"main",
                ShaderArtifactPath::resolve("renderer", "post/accum.slang").as_str(),
            );

        Self { accum_pass }
    }

    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        self.accum_pass.destroy(ctx);
    }

    pub fn exec(&self, cmd: &GfxCommandBuffer, data: AccumPassData, record_ctx: &RenderPassRecordCtx<'_>) {
        let image_view = |handle| {
            record_ctx.gfx_resource_manager.get_image_view(handle).expect("AccumPass: image view not found").handle()
        };
        let image_info =
            |view| vec![vk::DescriptorImageInfo::default().image_layout(vk::ImageLayout::GENERAL).image_view(view)];
        let descriptor_writes = [
            AccumDescriptorBinding::single_frame_input().write_image(
                vk::DescriptorSet::null(),
                0,
                image_info(image_view(data.single_frame_image)),
            ),
            AccumDescriptorBinding::accum_output().write_image(
                vk::DescriptorSet::null(),
                0,
                image_info(image_view(data.accum_image)),
            ),
        ];
        let frame_label = record_ctx.frame_timing.frame_label();
        self.accum_pass.exec(
            cmd,
            frame_label,
            record_ctx.shader_bindings.global_descriptor_sets(),
            &descriptor_writes,
            &gpu::renderer::render_passes::post_accum::PushConstant {
                image_size: glam::uvec2(data.image_size.width, data.image_size.height).into(),
                accum_frames: data.accum_frames,
                _padding_: 0,
            },
            glam::uvec3(
                data.image_size.width.div_ceil(gpu::renderer::render_passes::post_accum::SHADER_X as u32),
                data.image_size.height.div_ceil(gpu::renderer::render_passes::post_accum::SHADER_Y as u32),
                1,
            ),
        );
    }
}

/// 累积 Pass 的 RenderGraph 封装
pub struct AccumRgPass<'a> {
    pub accum_pass: &'a AccumPass,

    pub record_ctx: RenderPassRecordCtx<'a>,

    /// 单帧 RT 输出（只读）
    pub single_frame_image: RgImageHandle,
    /// 累积结果（读写）
    pub accum_image: RgImageHandle,

    pub image_extent: vk::Extent2D,
    /// 调用方维护的累计样本数；0 表示本次直接覆盖历史图像。这个契约让同一个 shader
    /// 可以服务 realtime 或 offline 调度，但历史有效性必须由各 pipeline 自己判断。
    pub accum_frames: u32,
}

impl<'a> RgPass for AccumRgPass<'a> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        // 读取单帧 RT 输出
        builder.read_image(self.single_frame_image, RgImageState::STORAGE_READ_COMPUTE);
        // 读写累积图像
        builder.read_write_image(self.accum_image, RgImageState::STORAGE_READ_WRITE_COMPUTE);
    }

    fn execute(&self, ctx: &RgPassContext) {
        let single_frame_view_handle = ctx.get_image_view_handle(self.single_frame_image).unwrap();
        let accum_view_handle = ctx.get_image_view_handle(self.accum_image).unwrap();

        self.accum_pass.exec(
            ctx.cmd,
            AccumPassData {
                single_frame_image: single_frame_view_handle,
                accum_image: accum_view_handle,
                image_size: self.image_extent,
                accum_frames: self.accum_frames,
            },
            &self.record_ctx,
        );
    }
}
