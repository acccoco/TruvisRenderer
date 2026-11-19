//! SR/RR 在现有 RenderGraph command buffer 中录制，同一次 evaluate 消费同帧 constants 和图像。

use std::cell::Cell;

use ash::vk::{self, Handle};
use renderer_kit::targets::ImageTarget;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_render_graph::render_graph::{
    RenderGraphBuilder, RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext,
};
use truvis_streamline_binding::dlss;

use super::{VIEWPORT_ID, options::DlssFeature, temporal::DlssSrFrameConstants};

/// Streamline 输入资源在 evaluate 前的稳定状态。
///
/// Streamline 通过 resource tag 读取 color/depth/motion/GBuffer 输入，内部可能在 graphics /
/// compute 阶段访问这些图像，因此这里用 `ALL_COMMANDS + MEMORY_READ` 表达外部 opaque pass
/// 的保守读依赖。layout 必须和传给 `sl::Resource` 的 layout 保持一致。
const SL_INPUT_READ: RgImageState = RgImageState::new(
    vk::PipelineStageFlags2::ALL_COMMANDS,
    vk::AccessFlags2::MEMORY_READ,
    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
);

/// Streamline 输出由内部命令写入，后续 `hdr-to-sdr` 仍按 storage image 读取。
const SL_WRITE: RgImageState = RgImageState::new(
    vk::PipelineStageFlags2::ALL_COMMANDS,
    vk::AccessFlags2::from_raw(vk::AccessFlags2::MEMORY_READ.as_raw() | vk::AccessFlags2::MEMORY_WRITE.as_raw()),
    vk::ImageLayout::GENERAL,
);

/// RT 或其它生产者导入的本帧低分辨率输入。图像 owner 保留到 GPU 完成。
///
/// depth 使用 [0,1] 的 device depth，motion vector 使用像素单位，含相机运动但不含 jitter。
#[derive(Clone, Copy)]
pub struct DlssInputs
{
    pub color: RgImageHandle,
    pub depth: RgImageHandle,
    pub motion_vectors: RgImageHandle,
    pub rr: Option<DlssRrGuides>,
}

/// RR guides 与 depth 对应同一可见表面。normal.xyz 和 roughness.w 打包在同一图像。
#[derive(Clone, Copy)]
pub struct DlssRrGuides
{
    pub diffuse_albedo: RgImageHandle,
    pub specular_albedo: RgImageHandle,
    pub normal_roughness: RgImageHandle,
    pub specular_hit_distance: RgImageHandle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Evaluation
{
    NotScheduled,
    Scheduled,
    Recording,
    Succeeded,
}

/// 本帧的配置与资源快照。建图和 debug preview 共用 evaluate 结果，submit 后交回 DLSS owner。
pub struct DlssFrame
{
    frame_index: u32,
    feature: Option<DlssFeature>,
    constants: DlssSrFrameConstants,
    output: ImageTarget,
    exposure: ImageTarget,
    evaluation: Cell<Evaluation>,
}

impl DlssFrame
{
    pub(super) fn new(
        frame_index: u32,
        feature: Option<DlssFeature>,
        constants: DlssSrFrameConstants,
        output: ImageTarget,
        exposure: ImageTarget,
    ) -> Self
    {
        Self {
            frame_index,
            feature,
            constants,
            output,
            exposure,
            evaluation: Cell::new(Evaluation::NotScheduled),
        }
    }

    pub fn sampling_jitter_px(&self) -> [f32; 2]
    {
        self.constants.sampling_jitter_offset
    }

    /// 在生成完当前帧输入后加入一个 DLSS pass。输出为线性 HDR，供图内后续 pass 使用。
    pub fn add_pass<'a>(
        &'a self,
        graph: &mut RenderGraphBuilder<'a>,
        resource_ctx: GfxResourceCtx<'a>,
        inputs: DlssInputs,
    ) -> Option<(RgImageHandle, vk::Extent2D)>
    {
        let feature = self.feature?;
        assert_eq!(self.evaluation.get(), Evaluation::NotScheduled, "DLSS pass already scheduled");
        let output = graph.import_image(
            "dlss-output",
            self.output.image,
            Some(self.output.view),
            self.output.format,
            RgImageState::UNDEFINED_TOP,
            None,
        );
        match feature {
            DlssFeature::SuperResolution => {
                let target = self.exposure;
                let exposure = graph.import_image(
                    "dlss-sr-exposure",
                    target.image,
                    Some(target.view),
                    target.format,
                    SL_INPUT_READ,
                    None,
                );
                graph.add_pass(
                    "dlss-sr",
                    DlssSrPass {
                        frame: self,
                        resource_ctx,
                        inputs,
                        output,
                        exposure,
                    },
                );
            }
            DlssFeature::RayReconstruction => {
                let guides = inputs.rr.expect("DLSS RR requires all guides");
                graph.add_pass(
                    "dlss-rr",
                    DlssRrPass {
                        frame: self,
                        resource_ctx,
                        inputs,
                        output,
                        guides,
                    },
                );
            }
        }
        self.evaluation.set(Evaluation::Scheduled);
        Some((output, self.output.extent))
    }

    /// 在 compute graph 录制后返回输入的稳定状态。未执行 DLSS 时沿用生产者的状态。
    pub fn input_state(&self, producer_state: RgImageState) -> RgImageState
    {
        if self.evaluation.get() == Evaluation::Succeeded { SL_INPUT_READ } else { producer_state }
    }

    /// 只有 RR 会读取额外的 guides。
    pub fn rr_guide_state(&self, producer_state: RgImageState) -> RgImageState
    {
        if self.feature == Some(DlssFeature::RayReconstruction) {
            self.input_state(producer_state)
        } else {
            producer_state
        }
    }

    /// compute 录制后只借出当帧的有效输出。SDR 读过输出后仍为 GENERAL。
    pub fn preview_output(&self) -> Option<(ImageTarget, RgImageState)>
    {
        (self.evaluation.get() == Evaluation::Succeeded).then_some((self.output, RgImageState::GENERAL))
    }

    pub(super) fn begin_recording(&self) -> Option<DlssFeature>
    {
        match self.evaluation.get() {
            Evaluation::NotScheduled => None,
            Evaluation::Scheduled => {
                self.evaluation.set(Evaluation::Recording);
                self.feature
            }
            _ => panic!("DLSS graph recording already started"),
        }
    }

    pub(super) fn evaluated_for_submit(&self) -> bool
    {
        match self.evaluation.get() {
            Evaluation::NotScheduled => false,
            Evaluation::Succeeded => true,
            _ => panic!("scheduled DLSS pass did not evaluate before submit"),
        }
    }

    fn assert_recording(&self)
    {
        assert_eq!(self.evaluation.get(), Evaluation::Recording, "DLSS feature was not registered before recording");
    }

    fn image_resource(
        ctx: &RgPassContext<'_>,
        resource_ctx: GfxResourceCtx<'_>,
        handle: RgImageHandle,
        state: RgImageState,
    ) -> dlss::ImageResource
    {
        let (image, view) = ctx.get_image_and_view(handle).expect("DLSS graph image or view missing");
        let extent = image.extent();
        // tag 的 Vulkan layout 与 setup 声明取自同一个 RgImageState。
        dlss::ImageResource {
            image: image.handle().as_raw(),
            memory: image.device_memory(resource_ctx).as_raw(),
            image_view: view.handle().as_raw(),
            layout: state.layout.as_raw() as u32,
            format: image.format().as_raw() as u32,
            width: extent.width,
            height: extent.height,
            mip_levels: 1,
            array_layers: 1,
            flags: image.flags().as_raw(),
            usage: image.usage().as_raw(),
        }
    }
}

impl DlssInputs
{
    fn declare(&self, builder: &mut RgPassBuilder, output: RgImageHandle)
    {
        builder.read_image(self.color, SL_INPUT_READ);
        builder.read_image(self.depth, SL_INPUT_READ);
        builder.read_image(self.motion_vectors, SL_INPUT_READ);
        builder.write_image(output, SL_WRITE);
    }
}

struct DlssSrPass<'a>
{
    frame: &'a DlssFrame,
    resource_ctx: GfxResourceCtx<'a>,
    inputs: DlssInputs,
    output: RgImageHandle,
    exposure: RgImageHandle,
}

impl RgPass for DlssSrPass<'_>
{
    fn setup(&mut self, builder: &mut RgPassBuilder)
    {
        self.inputs.declare(builder, self.output);
        builder.read_image(self.exposure, SL_INPUT_READ);
    }

    fn execute(&self, ctx: &RgPassContext<'_>)
    {
        self.frame.assert_recording();
        let read = |handle| DlssFrame::image_resource(ctx, self.resource_ctx, handle, SL_INPUT_READ);
        let desc = dlss::DlssEvaluateDesc {
            frame_index: self.frame.frame_index,
            viewport_id: VIEWPORT_ID,
            command_buffer: ctx.cmd.vk_handle().as_raw(),
            constants: self.frame.constants.to_streamline(),
            input_color: read(self.inputs.color),
            output_color: DlssFrame::image_resource(ctx, self.resource_ctx, self.output, SL_WRITE),
            depth_or_linear_depth: read(self.inputs.depth),
            motion_vectors: read(self.inputs.motion_vectors),
            exposure: read(self.exposure),
            use_linear_depth: false,
        };
        ctx.cmd.begin_label("DLSS SR", glam::vec4(0.25, 0.6, 1.0, 1.0));
        // SAFETY: RenderThread 串行录制，graph 已转换 layout，各 owner 保留图像至 GPU 使用完成。
        if let Err(err) = unsafe { dlss::evaluate(desc) } {
            log::error!("DLSS SR evaluate failed: {err}");
            panic!("DLSS SR evaluate failed: {err}");
        }
        ctx.cmd.end_label();
        self.frame.evaluation.set(Evaluation::Succeeded);
    }
}

struct DlssRrPass<'a>
{
    frame: &'a DlssFrame,
    resource_ctx: GfxResourceCtx<'a>,
    inputs: DlssInputs,
    output: RgImageHandle,
    guides: DlssRrGuides,
}

impl RgPass for DlssRrPass<'_>
{
    fn setup(&mut self, builder: &mut RgPassBuilder)
    {
        self.inputs.declare(builder, self.output);
        builder.read_image(self.guides.diffuse_albedo, SL_INPUT_READ);
        builder.read_image(self.guides.specular_albedo, SL_INPUT_READ);
        builder.read_image(self.guides.normal_roughness, SL_INPUT_READ);
        builder.read_image(self.guides.specular_hit_distance, SL_INPUT_READ);
    }

    fn execute(&self, ctx: &RgPassContext<'_>)
    {
        self.frame.assert_recording();
        let read = |handle| DlssFrame::image_resource(ctx, self.resource_ctx, handle, SL_INPUT_READ);
        let desc = dlss::DlssRrEvaluateDesc {
            frame_index: self.frame.frame_index,
            viewport_id: VIEWPORT_ID,
            command_buffer: ctx.cmd.vk_handle().as_raw(),
            constants: self.frame.constants.to_streamline(),
            input_color: read(self.inputs.color),
            output_color: DlssFrame::image_resource(ctx, self.resource_ctx, self.output, SL_WRITE),
            depth_or_linear_depth: read(self.inputs.depth),
            motion_vectors: read(self.inputs.motion_vectors),
            diffuse_albedo: read(self.guides.diffuse_albedo),
            specular_albedo: read(self.guides.specular_albedo),
            normal_roughness: read(self.guides.normal_roughness),
            specular_hit_distance: read(self.guides.specular_hit_distance),
            use_linear_depth: false,
        };
        ctx.cmd.begin_label("DLSS RR", glam::vec4(0.35, 0.95, 0.75, 1.0));
        // SAFETY: 除 SR 的录制契约外，本帧已提交与 view 和 packed guides 配套的 RR options。
        if let Err(err) = unsafe { dlss::evaluate_rr(desc) } {
            log::error!("DLSS RR evaluate failed: {err}");
            panic!("DLSS RR evaluate failed: {err}");
        }
        ctx.cmd.end_label();
        self.frame.evaluation.set(Evaluation::Succeeded);
    }
}

impl DlssSrFrameConstants
{
    /// jitter_offset 已是给 Streamline 的回正偏移。
    fn to_streamline(&self) -> dlss::Constants
    {
        let value = self;
        dlss::Constants {
            camera_view_to_clip: value.camera_view_to_clip,
            clip_to_camera_view: value.clip_to_camera_view,
            clip_to_prev_clip: value.clip_to_prev_clip,
            prev_clip_to_clip: value.prev_clip_to_clip,
            jitter_offset: value.jitter_offset,
            mvec_scale: value.mvec_scale,
            camera_pos: value.camera_pos,
            camera_up: value.camera_up,
            camera_right: value.camera_right,
            camera_fwd: value.camera_fwd,
            camera_near: value.camera_near,
            camera_far: value.camera_far,
            camera_fov: value.camera_fov,
            camera_aspect_ratio: value.camera_aspect_ratio,
            motion_vectors_invalid_value: value.motion_vectors_invalid_value,
            depth_inverted: value.depth_inverted,
            camera_motion_included: value.camera_motion_included,
            motion_vectors_3d: value.motion_vectors_3d,
            reset: value.reset,
        }
    }
}
