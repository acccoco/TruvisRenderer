use ash::vk;

use truvis_descriptor_layout_macro::DescriptorBinding;
use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_gfx::utilities::descriptor_cursor::GfxDescriptorCursor;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::bindings::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;
use truvis_renderer_shader_binding::gpu;
use truvis_shader_manifest::ShaderArtifactPath;

use crate::compute_pass::ComputePass;

#[derive(DescriptorBinding)]
struct ExposureDescriptorBinding {
    #[binding = 0]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "COMPUTE"]
    #[count = 1]
    _source: (),
    #[binding = 1]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "COMPUTE"]
    #[count = 1]
    _histogram: (),
    #[binding = 2]
    #[descriptor_type = "STORAGE_IMAGE"]
    #[stage = "COMPUTE"]
    #[count = 1]
    _exposure: (),
}

#[derive(Clone, Copy)]
pub enum ExposureStage {
    Clear,
    Histogram,
    Reduce,
}

/// 同一测光算法的三个 kernel；只借用 image，资源和跨帧历史由 SdrPostProcess 拥有。
pub struct AutoExposurePass {
    kernels: [ComputePass<gpu::renderer::render_passes::auto_exposure::PushConstant, ExposureDescriptorBinding>; 3],
}

impl AutoExposurePass {
    pub fn new(ctx: GfxDeviceCtx<'_>, descriptors: &GlobalDescriptorSets) -> Self {
        let kernels = [
            "post/exposure_clear.slang",
            "post/exposure_histogram.slang",
            "post/exposure_reduce.slang",
        ]
        .map(|path| {
            ComputePass::new(
                ctx,
                descriptors,
                gpu::renderer::render_passes::auto_exposure::SET_NUM,
                c"main",
                ShaderArtifactPath::resolve("renderer", path).as_str(),
            )
        });
        Self { kernels }
    }
    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        for kernel in self.kernels {
            kernel.destroy(ctx);
        }
    }
}

pub struct AutoExposureRgPass<'a> {
    pub pass: &'a AutoExposurePass,
    pub stage: ExposureStage,
    pub record_ctx: RenderPassRecordCtx<'a>,
    pub source: RgImageHandle,
    pub histogram: RgImageHandle,
    pub exposure: RgImageHandle,
    pub params: gpu::renderer::render_passes::auto_exposure::PushConstant,
}

impl RgPass for AutoExposureRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        match self.stage {
            ExposureStage::Clear => {
                builder.write_image(self.histogram, RgImageState::STORAGE_WRITE_COMPUTE);
            }
            ExposureStage::Histogram => {
                builder.read_image(self.source, RgImageState::STORAGE_READ_COMPUTE);
                builder.write_image(self.histogram, RgImageState::STORAGE_READ_WRITE_COMPUTE);
            }
            ExposureStage::Reduce => {
                builder.read_image(self.histogram, RgImageState::STORAGE_READ_COMPUTE);
                builder.write_image(self.exposure, RgImageState::STORAGE_READ_WRITE_COMPUTE);
            }
        }
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        let bindings = [
            ExposureDescriptorBinding::source(),
            ExposureDescriptorBinding::histogram(),
            ExposureDescriptorBinding::exposure(),
        ];
        let handles = [self.source, self.histogram, self.exposure];
        let writes: Vec<_> = bindings
            .into_iter()
            .zip(handles)
            .map(|(binding, handle)| {
                let view = ctx.get_image_view_handle(handle).unwrap();
                binding.write_image(
                    vk::DescriptorSet::null(),
                    0,
                    vec![
                        vk::DescriptorImageInfo::default()
                            .image_layout(vk::ImageLayout::GENERAL)
                            .image_view(self.record_ctx.gfx_resource_registry.get_image_view(view).unwrap().handle()),
                    ],
                )
            })
            .collect();
        let dispatch = match self.stage {
            ExposureStage::Histogram => {
                glam::uvec3(self.params.image_size.x.div_ceil(16), self.params.image_size.y.div_ceil(16), 1)
            }
            _ => glam::UVec3::ONE,
        };
        self.pass.kernels[self.stage as usize].exec(
            ctx.cmd,
            self.record_ctx.frame_timing.frame_label(),
            self.record_ctx.shader_bindings.global_descriptor_sets(),
            &writes,
            &self.params,
            dispatch,
        );
    }
}
