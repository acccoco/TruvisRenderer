//! Streamline opaque pass 共享工具。
//!
//! SR 与 RR 都在项目已有 command buffer 中调用 Streamline evaluate；RenderGraph 只负责
//! 在调用前把图像转换到 tag 中声明的 layout，并在调用后把输出留给后续 pass 使用。

use ash::vk;
use ash::vk::Handle;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageView;
use truvis_render_graph::render_graph::RgImageState;
use truvis_render_runtime::state::dlss_sr::{DlssSrFrameConstants, DlssSrMode};
use truvis_streamline_binding::dlss;

/// Streamline 输入资源在 evaluate 前的稳定状态。
///
/// Streamline 通过 resource tag 读取 color/depth/motion/GBuffer 输入，内部可能在 graphics /
/// compute 阶段访问这些图像，因此这里用 `ALL_COMMANDS + MEMORY_READ` 表达外部 opaque pass
/// 的保守读依赖。layout 必须和传给 `sl::Resource` 的 layout 保持一致。
pub const SL_INPUT_READ: RgImageState = RgImageState::new(
    vk::PipelineStageFlags2::ALL_COMMANDS,
    vk::AccessFlags2::MEMORY_READ,
    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
);

/// Streamline 输出由内部命令写入，后续 `hdr-to-sdr` 仍按 storage image 读取。
pub const SL_WRITE: RgImageState = RgImageState::new(
    vk::PipelineStageFlags2::ALL_COMMANDS,
    vk::AccessFlags2::from_raw(vk::AccessFlags2::MEMORY_READ.as_raw() | vk::AccessFlags2::MEMORY_WRITE.as_raw()),
    vk::ImageLayout::GENERAL,
);

pub fn image_resource(
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

pub fn to_streamline_constants(value: DlssSrFrameConstants) -> dlss::Constants {
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

pub fn to_streamline_mode(mode: DlssSrMode) -> dlss::DlssMode {
    match mode {
        DlssSrMode::Off => dlss::DlssMode::Off,
        DlssSrMode::Dlaa => dlss::DlssMode::Dlaa,
        DlssSrMode::Quality => dlss::DlssMode::Quality,
        DlssSrMode::Balanced => dlss::DlssMode::Balanced,
        DlssSrMode::Performance => dlss::DlssMode::Performance,
        DlssSrMode::UltraPerformance => dlss::DlssMode::UltraPerformance,
    }
}
