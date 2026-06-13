use ash::vk;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_path::TruvisPath;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::bindings::bindless_manager::BindlessUavHandle;
use truvis_render_runtime::bindings::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;
use truvis_shader_binding::gpu;

use crate::compute_pass::ComputePass;

pub struct BlitPassData {
    pub src_bindless_uav_handle: BindlessUavHandle,
    pub dst_bindless_uav_handle: BindlessUavHandle,

    pub src_image_size: vk::Extent2D,
    pub dst_image_size: vk::Extent2D,
}

pub struct BlitPass {
    blit_pass: ComputePass<gpu::ui_blit::PushConstant>,
}
impl BlitPass {
    pub fn new(ctx: GfxDeviceCtx<'_>, render_descriptor_sets: &GlobalDescriptorSets) -> Self {
        let blit_pass = ComputePass::<gpu::ui_blit::PushConstant>::new(
            ctx,
            render_descriptor_sets,
            c"main",
            TruvisPath::shader_build_path_str("ui/blit.slang").as_str(),
        );

        Self { blit_pass }
    }

    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        self.blit_pass.destroy(ctx);
    }

    pub fn exec(&self, cmd: &GfxCommandBuffer, data: BlitPassData, record_ctx: &RenderPassRecordCtx<'_>) {
        let frame_label = record_ctx.frame_timing.frame_label();
        self.blit_pass.exec(
            cmd,
            frame_label,
            record_ctx.shader_bindings.global_descriptor_sets(),
            &gpu::ui_blit::PushConstant {
                src_image: data.src_bindless_uav_handle.0,
                dst_image: data.dst_bindless_uav_handle.0,
                src_image_size: glam::uvec2(data.src_image_size.width, data.dst_image_size.height).into(),
                offset: glam::uvec2(0, 0).into(),
            },
            glam::uvec3(
                data.dst_image_size.width.div_ceil(gpu::ui_blit::SHADER_X as u32),
                data.dst_image_size.height.div_ceil(gpu::ui_blit::SHADER_Y as u32),
                1,
            ),
        );
    }
}

pub struct BlitRgPass<'a> {
    pub blit_pass: &'a BlitPass,

    pub record_ctx: RenderPassRecordCtx<'a>,

    pub src_image: RgImageHandle,
    pub dst_image: RgImageHandle,

    pub src_image_extent: vk::Extent2D,
    pub dst_image_extent: vk::Extent2D,
}
impl<'a> RgPass for BlitRgPass<'a> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        builder.read_image(self.src_image, RgImageState::STORAGE_READ_COMPUTE);
        builder.write_image(self.dst_image, RgImageState::STORAGE_WRITE_COMPUTE);
    }

    fn execute(&self, ctx: &RgPassContext) {
        let src_image_handle = ctx.get_image_view_handle(self.src_image).unwrap();
        let dst_image_handle = ctx.get_image_view_handle(self.dst_image).unwrap();
        let src_bindless_uav_handle = self.record_ctx.shader_bindings.get_shader_uav_handle(src_image_handle);
        let dst_bindless_uav_handle = self.record_ctx.shader_bindings.get_shader_uav_handle(dst_image_handle);

        self.blit_pass.exec(
            ctx.cmd,
            BlitPassData {
                src_bindless_uav_handle,
                dst_bindless_uav_handle,
                src_image_size: self.src_image_extent,
                dst_image_size: self.dst_image_extent,
            },
            &self.record_ctx,
        );
    }
}
