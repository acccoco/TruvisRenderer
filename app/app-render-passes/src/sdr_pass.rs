use ash::vk;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_path::TruvisPath;
use truvis_render_foundation::handles::GfxImageViewHandle;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::bindings::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;
use truvis_shader_binding::gpu;

use crate::compute_pass::ComputePass;

/// SDR 输出路径的 tone mapping 参数。
///
/// 这些参数只服务当前 RT pipeline 的 SDR sRGB 显示映射。曲线使用实时渲染常用的
/// ACES fitted approximation，不表达完整 ACES / OCIO / HDR10 display transform。
#[derive(Clone, Copy)]
pub struct SdrToneMappingSettings {
    pub exposure_ev: f32,
    pub aces_strength: f32,
    pub aces_white_point: f32,
}

impl Default for SdrToneMappingSettings {
    fn default() -> Self {
        Self {
            exposure_ev: 0.0,
            aces_strength: 1.0,
            aces_white_point: 11.2,
        }
    }
}

pub struct SdrPassData {
    pub src_image: GfxImageViewHandle,
    pub src_image_size: vk::Extent2D,

    pub dst_image: GfxImageViewHandle,
    pub dst_image_size: vk::Extent2D,
    /// 当前 RT 调试通道。0 使用 tone mapping；非 0 通道保留 HDR debug color。
    pub debug_channel: u32,
    pub tone_mapping: SdrToneMappingSettings,
}

pub struct SdrPass {
    sdr_pass: ComputePass<gpu::sdr::PushConstant>,
}
impl SdrPass {
    pub fn new(ctx: GfxDeviceCtx<'_>, render_descriptor_sets: &GlobalDescriptorSets) -> Self {
        let sdr_pass = ComputePass::<gpu::sdr::PushConstant>::new(
            ctx,
            render_descriptor_sets,
            c"main",
            TruvisPath::shader_build_path_str("post/sdr.slang").as_str(),
        );

        Self { sdr_pass }
    }

    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        self.sdr_pass.destroy(ctx);
    }

    pub fn exec(&self, cmd: &GfxCommandBuffer, data: SdrPassData, record_ctx: &RenderPassRecordCtx<'_>) {
        let src_image_bindless_handle = record_ctx.shader_bindings.get_shader_uav_handle(data.src_image);
        let dst_image_bindless_handle = record_ctx.shader_bindings.get_shader_uav_handle(data.dst_image);

        let frame_label = record_ctx.frame_timing.frame_label();
        self.sdr_pass.exec(
            cmd,
            frame_label,
            record_ctx.shader_bindings.global_descriptor_sets(),
            &gpu::sdr::PushConstant {
                src_image: src_image_bindless_handle.0,
                dst_image: dst_image_bindless_handle.0,
                image_size: glam::uvec2(data.src_image_size.width, data.src_image_size.height).into(),
                channel: data.debug_channel,
                exposure_ev: data.tone_mapping.exposure_ev,
                aces_strength: data.tone_mapping.aces_strength,
                aces_white_point: data.tone_mapping.aces_white_point,
                _padding_1: Default::default(),
            },
            glam::uvec3(
                data.dst_image_size.width.div_ceil(gpu::sdr::SHADER_X as u32),
                data.dst_image_size.height.div_ceil(gpu::sdr::SHADER_Y as u32),
                1,
            ),
        );
    }
}

pub struct SdrRgPass<'a> {
    pub sdr_pass: &'a SdrPass,

    pub record_ctx: RenderPassRecordCtx<'a>,

    pub src_image: RgImageHandle,
    pub dst_image: RgImageHandle,

    pub src_image_extent: vk::Extent2D,
    pub dst_image_extent: vk::Extent2D,
    pub debug_channel: u32,
    pub tone_mapping: SdrToneMappingSettings,
}
impl<'a> RgPass for SdrRgPass<'a> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
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
                debug_channel: self.debug_channel,
                tone_mapping: self.tone_mapping,
            },
            &self.record_ctx,
        );
    }
}
