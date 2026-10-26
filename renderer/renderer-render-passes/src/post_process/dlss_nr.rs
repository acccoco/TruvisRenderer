//! NR 外部 evaluate 与显示解码；只借用会话/图像，不提交、不等待，不接管 owner。

use std::cell::{Cell, RefCell};

use ash::vk::{self, Handle};

use truvis_descriptor_layout_macro::DescriptorBinding;
use truvis_dlss_nr_binding::{EvaluateDesc, Image, NrSession};
use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_gfx::utilities::descriptor_cursor::GfxDescriptorCursor;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::bindings::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;
use truvis_renderer_shader_binding::gpu;
use truvis_shader_manifest::ShaderArtifactPath;

use crate::compute_pass::ComputePass;
use crate::post_process::dlss_options::DlssEvaluation;

/// 引导图由 Realtime 持有，extent 是内部渲染尺寸；不在 adapter 中重采样。
#[derive(Clone, Copy)]
pub struct NrGuides {
    pub depth: RgImageHandle,
    pub motion_vectors: RgImageHandle,
    pub extent: vk::Extent2D,
}

#[derive(Default, Debug)]
pub enum NrEvaluation {
    #[default]
    NotRun,
    Succeeded,
    Failed(String),
}

impl NrEvaluation {
    pub fn check(&self) -> Result<(), String> {
        match self {
            Self::Failed(error) => Err(error.clone()),
            _ => Ok(()),
        }
    }
}

/// RefCell 的借用只覆盖一次同步录制；NrSession 的 !Send/!Sync 保持 RenderThread 边界。
pub struct NrRgPass<'a> {
    pub session: &'a RefCell<NrSession>,
    pub evaluation: &'a RefCell<NrEvaluation>,
    pub upstream: &'a Cell<DlssEvaluation>,
    pub upstream_required: bool,
    pub color: RgImageHandle,
    pub output: RgImageHandle,
    pub mask: RgImageHandle,
    pub guides: NrGuides,
    pub reset: bool,
    pub jitter: [f32; 2],
    pub intensity: f32,
}

impl NrRgPass<'_> {
    /// SDK 内部 stage 不透明；跨帧输出复用使用相同保守读写状态。
    pub const READ: RgImageState = RgImageState::new(
        vk::PipelineStageFlags2::ALL_COMMANDS,
        vk::AccessFlags2::MEMORY_READ,
        vk::ImageLayout::GENERAL,
    );
    pub const WRITE: RgImageState = RgImageState::new(
        vk::PipelineStageFlags2::ALL_COMMANDS,
        vk::AccessFlags2::from_raw(vk::AccessFlags2::MEMORY_READ.as_raw() | vk::AccessFlags2::MEMORY_WRITE.as_raw()),
        vk::ImageLayout::GENERAL,
    );

    fn image(ctx: &RgPassContext<'_>, handle: RgImageHandle) -> Image {
        let (image, view) = ctx.get_image_and_view(handle).expect("NR graph image missing");
        let extent = image.extent();
        Image {
            image: image.handle().as_raw(),
            image_view: view.handle().as_raw(),
            width: extent.width,
            height: extent.height,
            format: image.format().as_raw() as u32,
            layout: vk::ImageLayout::GENERAL.as_raw() as u32,
        }
    }
}

impl RgPass for NrRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        for input in [self.color, self.mask, self.guides.depth, self.guides.motion_vectors] {
            builder.read_image(input, Self::READ);
        }
        builder.write_image(self.output, Self::WRITE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        // 上游失败时不能把无效颜色交给 NGX；原始 Streamline 错误已在对应 adapter 中记录。
        if self.upstream_required && self.upstream.get() != DlssEvaluation::Succeeded {
            *self.evaluation.borrow_mut() = NrEvaluation::Failed(
                "NR input unavailable: preceding SR/RR evaluate failed; see original Streamline error".into(),
            );
            return;
        }
        let desc = EvaluateDesc {
            color: Self::image(ctx, self.color),
            output: Self::image(ctx, self.output),
            depth: Self::image(ctx, self.guides.depth),
            motion_vectors: Self::image(ctx, self.guides.motion_vectors),
            control_mask: Self::image(ctx, self.mask),
            reset: self.reset as u32,
            jitter_x: self.jitter[0],
            jitter_y: self.jitter[1],
            motion_scale_x: 1.0,
            motion_scale_y: 1.0,
            intensity: self.intensity,
        };
        if desc.depth.width != self.guides.extent.width || desc.depth.height != self.guides.extent.height {
            *self.evaluation.borrow_mut() = NrEvaluation::Failed("NR guide extent differs from current Realtime frame".into());
            return;
        }
        let result = unsafe { self.session.borrow_mut().evaluate(ctx.cmd.vk_handle().as_raw(), &desc) };
        *self.evaluation.borrow_mut() = match result {
            Ok(()) => NrEvaluation::Succeeded,
            Err(error) => {
                log::error!("NR evaluate failed: {error}");
                NrEvaluation::Failed(error.to_string())
            }
        };
    }
}

#[derive(DescriptorBinding)]
struct NrDecodeBindings {
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
}

pub struct NrDecodePass {
    compute: ComputePass<gpu::renderer::render_passes::nr_decode::PushConstant, NrDecodeBindings>,
}

impl NrDecodePass {
    pub fn new(device: GfxDeviceCtx<'_>, sets: &GlobalDescriptorSets) -> Self {
        Self {
            compute: ComputePass::new(
                device,
                sets,
                gpu::renderer::render_passes::nr_decode::SET_NUM,
                c"main",
                ShaderArtifactPath::resolve("renderer", "post/nr_decode.slang").as_str(),
            ),
        }
    }
    pub fn destroy(self, device: GfxDeviceCtx<'_>) {
        self.compute.destroy(device);
    }
}

pub struct NrDecodeRgPass<'a> {
    pub pass: &'a NrDecodePass,
    pub evaluation: &'a RefCell<NrEvaluation>,
    pub record_ctx: RenderPassRecordCtx<'a>,
    pub source: RgImageHandle,
    pub destination: RgImageHandle,
    pub extent: vk::Extent2D,
    pub dither: bool,
}

impl RgPass for NrDecodeRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        builder.read_image(self.source, RgImageState::STORAGE_READ_COMPUTE);
        builder.write_image(self.destination, RgImageState::STORAGE_WRITE_COMPUTE);
    }
    fn execute(&self, ctx: &RgPassContext<'_>) {
        if !matches!(*self.evaluation.borrow(), NrEvaluation::Succeeded) {
            return;
        }
        let source = ctx.get_image_and_view(self.source).unwrap().1.handle();
        let destination = ctx.get_image_and_view(self.destination).unwrap().1.handle();
        let writes = [
            NrDecodeBindings::src_image().write_image(
                vk::DescriptorSet::null(),
                0,
                vec![vk::DescriptorImageInfo::default().image_layout(vk::ImageLayout::GENERAL).image_view(source)],
            ),
            NrDecodeBindings::dst_image().write_image(
                vk::DescriptorSet::null(),
                0,
                vec![vk::DescriptorImageInfo::default().image_layout(vk::ImageLayout::GENERAL).image_view(destination)],
            ),
        ];
        self.pass.compute.exec(
            ctx.cmd,
            self.record_ctx.frame_timing.frame_label(),
            self.record_ctx.shader_bindings.global_descriptor_sets(),
            &writes,
            &gpu::renderer::render_passes::nr_decode::PushConstant {
                image_size: glam::uvec2(self.extent.width, self.extent.height).into(),
                dither: self.dither as u32,
                _padding_0: 0,
            },
            glam::uvec3(self.extent.width.div_ceil(8), self.extent.height.div_ceil(8), 1),
        );
    }
}
