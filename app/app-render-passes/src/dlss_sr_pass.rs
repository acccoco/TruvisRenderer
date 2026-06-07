//! DLSS Super Resolution 的 RenderGraph adapter。
//!
//! 本 pass 不执行项目 shader，而是在现有 Vulkan command buffer 中调用 Streamline。
//! RenderGraph 只负责把输入/输出图像转到 Streamline 期望的 layout，并保证 evaluate
//! 发生在 ray tracing 之后、SDR pass 之前。

use ash::vk;
use ash::vk::Handle;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageView;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;
use truvis_render_runtime::state::dlss_sr::DlssSrFrameConstants;
use truvis_render_runtime::state::dlss_sr::DlssSrMode;
use truvis_streamline_binding::dlss;

/// DLSS SR 输入资源在 evaluate 前的稳定状态。
///
/// Streamline 通过 resource tag 读取 color/depth/motion vectors，内部可能在 graphics /
/// compute 阶段访问这些图像，因此这里用 `ALL_COMMANDS + MEMORY_READ` 表达外部 opaque pass
/// 的保守读依赖。layout 必须和传给 `sl::Resource` 的 layout 保持一致。
pub const DLSS_SR_INPUT_READ: RgImageState = RgImageState::new(
    vk::PipelineStageFlags2::ALL_COMMANDS,
    vk::AccessFlags2::MEMORY_READ,
    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
);

/// DLSS SR 输出由 Streamline 内部写入，后续 `hdr-to-sdr` 仍按 storage image 读取。
///
/// 这里保持 `GENERAL`，避免 SR output 在 opaque pass 和项目 compute pass 之间出现
/// image layout 契约分裂。
const DLSS_WRITE: RgImageState = RgImageState::new(
    vk::PipelineStageFlags2::ALL_COMMANDS,
    vk::AccessFlags2::from_raw(vk::AccessFlags2::MEMORY_READ.as_raw() | vk::AccessFlags2::MEMORY_WRITE.as_raw()),
    vk::ImageLayout::GENERAL,
);

/// Streamline DLSS SR 的无状态 pass owner。
///
/// Streamline feature resource 生命周期由全局 runtime 与 viewport 管理；本结构只负责每帧
/// 组装 options/constants/resource tags 并在当前 command buffer 上调用 evaluate。
pub struct DlssSrPass;

impl DlssSrPass {
    pub fn new() -> Self {
        Self
    }

    pub fn destroy(self) {}

    /// 在当前 command buffer 中执行一次 DLSS SR evaluate。
    ///
    /// 约定：
    /// - viewport 当前固定为 0，mode/resize 时由 runtime 负责 reset/free resources；
    /// - 输入 color/depth/mvec 已由 RenderGraph 转到 `SHADER_READ_ONLY_OPTIMAL`；
    /// - output color 已由 RenderGraph 转到 `GENERAL`；
    /// - 失败只记录日志，不在 pass 内切换 fallback，避免在录制中的 graph 改变执行分支。
    pub fn evaluate(
        &self,
        cmd: &GfxCommandBuffer,
        record_ctx: &RenderPassRecordCtx<'_>,
        resource_ctx: GfxResourceCtx<'_>,
        data: DlssSrPassData<'_>,
    ) {
        let mode = record_ctx.render_options.dlss_sr_mode;
        if mode == DlssSrMode::Off {
            return;
        }

        let output_extent = record_ctx.frame_state.output_extent;
        // options 必须与 runtime 计算出的 output extent 一致；render extent 则来自输入图像尺寸。
        let options = dlss::DlssOptions {
            mode: to_streamline_mode(mode),
            output_width: output_extent.width,
            output_height: output_extent.height,
            color_buffers_hdr: true,
        };
        if let Err(err) = dlss::set_options(0, options) {
            log::error!("DLSS SR set options failed: {}", err);
            return;
        }

        let constants = to_streamline_constants(record_ctx.dlss_sr_state.constants());
        // `ImageResource` 中的 layout/format/usage 会被 Streamline 作为 Vulkan resource tag 契约读取。
        // 这里不要临时推断 layout，必须和 `setup()` 中声明的 RenderGraph 状态同步维护。
        let desc = dlss::DlssEvaluateDesc {
            frame_index: record_ctx.frame_timing.frame_id() as u32,
            viewport_id: 0,
            command_buffer: cmd.vk_handle().as_raw(),
            constants,
            input_color: image_resource(
                resource_ctx,
                data.input_color,
                data.input_color_view,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ),
            output_color: image_resource(
                resource_ctx,
                data.output_color,
                data.output_color_view,
                vk::ImageLayout::GENERAL,
            ),
            depth_or_linear_depth: image_resource(
                resource_ctx,
                data.depth,
                data.depth_view,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ),
            motion_vectors: image_resource(
                resource_ctx,
                data.motion_vectors,
                data.motion_vectors_view,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ),
            use_linear_depth: false,
        };

        cmd.begin_label("DLSS SR", glam::vec4(0.25, 0.6, 1.0, 1.0));
        if let Err(err) = dlss::evaluate(desc) {
            log::error!("DLSS SR evaluate failed: {}", err);
        }
        cmd.end_label();
    }
}

/// DLSS SR evaluate 所需的真实 Vulkan 图像引用。
///
/// 这里直接持有 `GfxImage` / `GfxImageView` 引用，是为了把 memory、usage、format、extent
/// 一并传给 Streamline；RenderGraph handle 到真实资源的解析只发生在 adapter 层。
pub struct DlssSrPassData<'a> {
    pub input_color: &'a GfxImage,
    pub input_color_view: &'a GfxImageView,
    pub output_color: &'a GfxImage,
    pub output_color_view: &'a GfxImageView,
    pub depth: &'a GfxImage,
    pub depth_view: &'a GfxImageView,
    pub motion_vectors: &'a GfxImage,
    pub motion_vectors_view: &'a GfxImageView,
}

/// RenderGraph 中的 DLSS SR pass 声明。
///
/// 该 adapter 的职责是声明图像状态并把 `RgImageHandle` 解析成 `DlssSrPassData`。
/// 它不拥有 Streamline runtime，也不决定当前是否启用 SR；执行分支由 RT pipeline 添加 pass 时决定。
pub struct DlssSrRgPass<'a> {
    pub dlss_sr_pass: &'a DlssSrPass,
    pub record_ctx: RenderPassRecordCtx<'a>,
    pub resource_ctx: GfxResourceCtx<'a>,
    pub input_color: RgImageHandle,
    pub output_color: RgImageHandle,
    pub depth: RgImageHandle,
    pub motion_vectors: RgImageHandle,
}

impl RgPass for DlssSrRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        // 三个输入必须和 `evaluate()` 中传给 Streamline 的 image layout 一致。
        builder.read_image(self.input_color, DLSS_SR_INPUT_READ);
        builder.read_image(self.depth, DLSS_SR_INPUT_READ);
        builder.read_image(self.motion_vectors, DLSS_SR_INPUT_READ);
        builder.write_image(self.output_color, DLSS_WRITE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        let (input_color, input_color_view) =
            ctx.get_image_and_view(self.input_color).expect("DlssSrRgPass: input_color not found");
        let (output_color, output_color_view) =
            ctx.get_image_and_view(self.output_color).expect("DlssSrRgPass: output_color not found");
        let (depth, depth_view) = ctx.get_image_and_view(self.depth).expect("DlssSrRgPass: depth not found");
        let (motion_vectors, motion_vectors_view) =
            ctx.get_image_and_view(self.motion_vectors).expect("DlssSrRgPass: motion_vectors not found");

        self.dlss_sr_pass.evaluate(
            ctx.cmd,
            &self.record_ctx,
            self.resource_ctx,
            DlssSrPassData {
                input_color,
                input_color_view,
                output_color,
                output_color_view,
                depth,
                depth_view,
                motion_vectors,
                motion_vectors_view,
            },
        );
    }
}

fn image_resource(
    resource_ctx: GfxResourceCtx<'_>,
    image: &GfxImage,
    image_view: &GfxImageView,
    layout: vk::ImageLayout,
) -> dlss::ImageResource {
    let extent = image.extent();
    // Streamline 需要原始 Vulkan handle、image memory 和 native format；这些信息不在
    // RenderGraph handle 中，因此必须从 manager-owned `GfxImage` 取快照。
    dlss::ImageResource {
        image: image.handle().as_raw(),
        memory: image.device_memory(resource_ctx).as_raw(),
        image_view: image_view.handle().as_raw(),
        layout: layout.as_raw() as u32,
        format: image.format().as_raw() as u32,
        width: extent.width,
        height: extent.height,
        mip_levels: 1,
        array_layers: 1,
        flags: image.flags().as_raw(),
        usage: image.usage().as_raw(),
    }
}

fn to_streamline_constants(value: DlssSrFrameConstants) -> dlss::Constants {
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

fn to_streamline_mode(mode: DlssSrMode) -> dlss::DlssMode {
    match mode {
        DlssSrMode::Off => dlss::DlssMode::Off,
        DlssSrMode::Dlaa => dlss::DlssMode::Dlaa,
        DlssSrMode::Quality => dlss::DlssMode::Quality,
        DlssSrMode::Balanced => dlss::DlssMode::Balanced,
        DlssSrMode::Performance => dlss::DlssMode::Performance,
        DlssSrMode::UltraPerformance => dlss::DlssMode::UltraPerformance,
    }
}
