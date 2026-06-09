//! DLSS Ray Reconstruction 的 RenderGraph adapter。
//!
//! RR 是 DLSS SR 基础设施上的替代 evaluate 分支：开启 RR 时调用 `kFeatureDLSS_RR`，
//! 不再追加普通 `kFeatureDLSS` SR pass，也不再运行 legacy denoise/accum。

use crate::streamline_pass::{SL_INPUT_READ, SL_WRITE, image_resource, to_streamline_constants, to_streamline_mode};
use ash::vk;
use ash::vk::Handle;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxResourceCtx;
use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageView;
use truvis_render_graph::render_graph::{RgImageHandle, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;
use truvis_render_runtime::state::dlss_sr::DlssSrMode;
use truvis_streamline_binding::dlss;

pub struct DlssRrPass;

impl DlssRrPass {
    pub fn new() -> Self {
        Self
    }

    pub fn destroy(self) {}

    pub fn evaluate(
        &self,
        cmd: &GfxCommandBuffer,
        record_ctx: &RenderPassRecordCtx<'_>,
        resource_ctx: GfxResourceCtx<'_>,
        data: DlssRrPassData<'_>,
    ) {
        let mode = record_ctx.render_options.dlss_sr_mode;
        if mode == DlssSrMode::Off || !record_ctx.render_options.dlss_rr_enabled {
            return;
        }

        let output_extent = record_ctx.frame_state.output_extent;
        let frame_constants = record_ctx.dlss_sr_state.constants();
        let options = dlss::DlssRrOptions {
            mode: to_streamline_mode(mode),
            output_width: output_extent.width,
            output_height: output_extent.height,
            color_buffers_hdr: true,
            normal_roughness_packed: true,
            world_to_camera_view: frame_constants.world_to_camera_view,
            camera_view_to_world: frame_constants.camera_view_to_world,
        };
        if let Err(err) = dlss::set_rr_options(0, options) {
            log::error!("DLSS RR set options failed: {}", err);
            return;
        }

        let desc = dlss::DlssRrEvaluateDesc {
            frame_index: record_ctx.frame_timing.frame_id() as u32,
            viewport_id: 0,
            command_buffer: cmd.vk_handle().as_raw(),
            constants: to_streamline_constants(frame_constants),
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
            diffuse_albedo: image_resource(
                resource_ctx,
                data.diffuse_albedo,
                data.diffuse_albedo_view,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ),
            specular_albedo: image_resource(
                resource_ctx,
                data.specular_albedo,
                data.specular_albedo_view,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ),
            normal_roughness: image_resource(
                resource_ctx,
                data.normal_roughness,
                data.normal_roughness_view,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ),
            specular_motion_vectors: image_resource(
                resource_ctx,
                data.specular_motion_vectors,
                data.specular_motion_vectors_view,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ),
            use_linear_depth: false,
        };

        cmd.begin_label("DLSS RR", glam::vec4(0.35, 0.95, 0.75, 1.0));
        if let Err(err) = dlss::evaluate_rr(desc) {
            log::error!("DLSS RR evaluate failed: {}", err);
        }
        cmd.end_label();
    }
}

pub struct DlssRrPassData<'a> {
    pub input_color: &'a GfxImage,
    pub input_color_view: &'a GfxImageView,
    pub output_color: &'a GfxImage,
    pub output_color_view: &'a GfxImageView,
    pub depth: &'a GfxImage,
    pub depth_view: &'a GfxImageView,
    pub motion_vectors: &'a GfxImage,
    pub motion_vectors_view: &'a GfxImageView,
    pub diffuse_albedo: &'a GfxImage,
    pub diffuse_albedo_view: &'a GfxImageView,
    pub specular_albedo: &'a GfxImage,
    pub specular_albedo_view: &'a GfxImageView,
    pub normal_roughness: &'a GfxImage,
    pub normal_roughness_view: &'a GfxImageView,
    pub specular_motion_vectors: &'a GfxImage,
    pub specular_motion_vectors_view: &'a GfxImageView,
}

pub struct DlssRrRgPass<'a> {
    pub dlss_rr_pass: &'a DlssRrPass,
    pub record_ctx: RenderPassRecordCtx<'a>,
    pub resource_ctx: GfxResourceCtx<'a>,
    pub input_color: RgImageHandle,
    pub output_color: RgImageHandle,
    pub depth: RgImageHandle,
    pub motion_vectors: RgImageHandle,
    pub diffuse_albedo: RgImageHandle,
    pub specular_albedo: RgImageHandle,
    pub normal_roughness: RgImageHandle,
    pub specular_motion_vectors: RgImageHandle,
}

impl RgPass for DlssRrRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        builder.read_image(self.input_color, SL_INPUT_READ);
        builder.read_image(self.depth, SL_INPUT_READ);
        builder.read_image(self.motion_vectors, SL_INPUT_READ);
        builder.read_image(self.diffuse_albedo, SL_INPUT_READ);
        builder.read_image(self.specular_albedo, SL_INPUT_READ);
        builder.read_image(self.normal_roughness, SL_INPUT_READ);
        builder.read_image(self.specular_motion_vectors, SL_INPUT_READ);
        builder.write_image(self.output_color, SL_WRITE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        let (input_color, input_color_view) =
            ctx.get_image_and_view(self.input_color).expect("DlssRrRgPass: input_color not found");
        let (output_color, output_color_view) =
            ctx.get_image_and_view(self.output_color).expect("DlssRrRgPass: output_color not found");
        let (depth, depth_view) = ctx.get_image_and_view(self.depth).expect("DlssRrRgPass: depth not found");
        let (motion_vectors, motion_vectors_view) =
            ctx.get_image_and_view(self.motion_vectors).expect("DlssRrRgPass: motion_vectors not found");
        let (diffuse_albedo, diffuse_albedo_view) =
            ctx.get_image_and_view(self.diffuse_albedo).expect("DlssRrRgPass: diffuse_albedo not found");
        let (specular_albedo, specular_albedo_view) =
            ctx.get_image_and_view(self.specular_albedo).expect("DlssRrRgPass: specular_albedo not found");
        let (normal_roughness, normal_roughness_view) =
            ctx.get_image_and_view(self.normal_roughness).expect("DlssRrRgPass: normal_roughness not found");
        let (specular_motion_vectors, specular_motion_vectors_view) = ctx
            .get_image_and_view(self.specular_motion_vectors)
            .expect("DlssRrRgPass: specular_motion_vectors not found");

        self.dlss_rr_pass.evaluate(
            ctx.cmd,
            &self.record_ctx,
            self.resource_ctx,
            DlssRrPassData {
                input_color,
                input_color_view,
                output_color,
                output_color_view,
                depth,
                depth_view,
                motion_vectors,
                motion_vectors_view,
                diffuse_albedo,
                diffuse_albedo_view,
                specular_albedo,
                specular_albedo_view,
                normal_roughness,
                normal_roughness_view,
                specular_motion_vectors,
                specular_motion_vectors_view,
            },
        );
    }
}
