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

pub use super::settings::SdrPostProcessSettings;

pub struct SdrPassData {
    pub src_image: GfxImageViewHandle,
    pub src_image_size: vk::Extent2D,

    pub dst_image: GfxImageViewHandle,
    pub dst_image_size: vk::Extent2D,
    /// CPU 唯一判定通道语义：radiance 使用显示映射，数据通道直接透传。
    pub radiance_channel: bool,
    pub post_process: SdrPostProcessSettings,
    pub exposure: GfxImageViewHandle,
    pub agx_lut: GfxImageViewHandle,
}

#[derive(DescriptorBinding)]
struct SdrDescriptorBinding {
    #[binding = 0]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "COMPUTE"]
    #[count = 1]
    _src_image: (),

    #[binding = 1]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "COMPUTE"]
    #[count = 1]
    _dst_image: (),
    #[binding = 2]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "COMPUTE"]
    #[count = 1]
    _exposure: (),
    #[binding = 3]
    #[descriptor_type = "SAMPLED_IMAGE"]
    #[stage = "COMPUTE"]
    #[count = 1]
    _agx_lut: (),
}

/// HDR 到 SDR 的 compute pass。
///
/// src/dst image 由当前 RenderGraph dispatch 通过 pass-local push descriptor 绑定；本 pass 不注册、
/// 缓存或拥有 image，因此 resize 只改变本次录制使用的 view，不产生全局 bindless slot 生命周期。
pub struct SdrPass {
    sdr_pass: ComputePass<gpu::renderer::render_passes::sdr::PushConstant, SdrDescriptorBinding>,
}
impl SdrPass {
    pub fn new(ctx: GfxDeviceCtx<'_>, render_descriptor_sets: &GlobalDescriptorSets) -> Self {
        let sdr_pass = ComputePass::<gpu::renderer::render_passes::sdr::PushConstant, SdrDescriptorBinding>::new(
            ctx,
            render_descriptor_sets,
            gpu::renderer::render_passes::sdr::SET_NUM,
            c"main",
            ShaderArtifactPath::resolve("renderer", "post/sdr.slang").as_str(),
        );

        Self { sdr_pass }
    }

    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        self.sdr_pass.destroy(ctx);
    }

    pub fn exec(&self, cmd: &GfxCommandBuffer, data: SdrPassData, record_ctx: &RenderPassRecordCtx<'_>) {
        let image_view = |handle| {
            record_ctx.gfx_resource_registry.get_image_view(handle).expect("SdrPass: image view not found").handle()
        };
        let image_info =
            |view| vec![vk::DescriptorImageInfo::default().image_layout(vk::ImageLayout::GENERAL).image_view(view)];
        let descriptor_writes = [
            SdrDescriptorBinding::src_image().write_image(
                vk::DescriptorSet::null(),
                0,
                image_info(image_view(data.src_image)),
            ),
            SdrDescriptorBinding::dst_image().write_image(
                vk::DescriptorSet::null(),
                0,
                image_info(image_view(data.dst_image)),
            ),
            SdrDescriptorBinding::exposure().write_image(
                vk::DescriptorSet::null(),
                0,
                image_info(image_view(data.exposure)),
            ),
            SdrDescriptorBinding::agx_lut().write_image(
                vk::DescriptorSet::null(),
                0,
                vec![
                    vk::DescriptorImageInfo::default()
                        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .image_view(image_view(data.agx_lut)),
                ],
            ),
        ];

        let grading = data.post_process.color_grading;
        let white_balance = grading.white_balance_matrix().transpose();
        let frame_label = record_ctx.frame_timing.frame_label();
        self.sdr_pass.exec(
            cmd,
            frame_label,
            record_ctx.shader_bindings.global_descriptor_sets(),
            &descriptor_writes,
            &gpu::renderer::render_passes::sdr::PushConstant {
                image_size: glam::uvec2(data.dst_image_size.width, data.dst_image_size.height).into(),
                src_size: glam::uvec2(data.src_image_size.width, data.src_image_size.height).into(),
                radiance_channel: data.radiance_channel as u32,
                tone_mapping_mode: data.post_process.tone_mapping as u32,
                exposure_mode: data.post_process.exposure.mode as u32,
                dither: data.post_process.dither as u32,
                manual_ev: data.post_process.exposure.manual_ev,
                auto_compensation_ev: data.post_process.exposure.auto_compensation_ev,
                contrast: grading.contrast,
                saturation: grading.saturation,
                white_balance_r: white_balance.x_axis.extend(0.0).into(),
                white_balance_g: white_balance.y_axis.extend(0.0).into(),
                white_balance_b: white_balance.z_axis.extend(0.0).into(),
                tonal_gains: grading.tonal_gains().into(),
                _padding_0: 0.0,
            },
            glam::uvec3(
                data.dst_image_size.width.div_ceil(gpu::renderer::render_passes::sdr::SHADER_X as u32),
                data.dst_image_size.height.div_ceil(gpu::renderer::render_passes::sdr::SHADER_Y as u32),
                1,
            ),
        );
    }
}

pub struct SdrRgPass<'a> {
    pub exposure: RgImageHandle,
    pub agx_lut: RgImageHandle,
    pub sdr_pass: &'a SdrPass,

    pub record_ctx: RenderPassRecordCtx<'a>,

    pub src_image: RgImageHandle,
    pub dst_image: RgImageHandle,

    pub src_image_extent: vk::Extent2D,
    pub dst_image_extent: vk::Extent2D,
    pub radiance_channel: bool,
    pub post_process: SdrPostProcessSettings,
}
impl<'a> RgPass for SdrRgPass<'a> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        builder.read_image(self.exposure, RgImageState::STORAGE_READ_COMPUTE);
        builder.read_image(
            self.agx_lut,
            RgImageState::new(
                vk::PipelineStageFlags2::COMPUTE_SHADER,
                vk::AccessFlags2::SHADER_SAMPLED_READ,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ),
        );
        builder.read_image(self.src_image, RgImageState::STORAGE_READ_COMPUTE);
        builder.write_image(self.dst_image, RgImageState::STORAGE_WRITE_COMPUTE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        let src_image = ctx.get_image_view_handle(self.src_image).unwrap();
        let dst_image = ctx.get_image_view_handle(self.dst_image).unwrap();

        self.sdr_pass.exec(
            ctx.cmd,
            SdrPassData {
                src_image,
                dst_image,
                src_image_size: self.src_image_extent,
                dst_image_size: self.dst_image_extent,
                radiance_channel: self.radiance_channel,
                exposure: ctx.get_image_view_handle(self.exposure).unwrap(),
                agx_lut: ctx.get_image_view_handle(self.agx_lut).unwrap(),
                post_process: self.post_process,
            },
            &self.record_ctx,
        );
    }
}
