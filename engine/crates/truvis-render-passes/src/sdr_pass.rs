use ash::vk;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_path::TruvisPath;
use truvis_render_graph::compute_pass::ComputePass;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_interface::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_interface::handles::GfxImageViewHandle;
use truvis_render_interface::render_world::RenderWorld;
use truvis_shader_binding::gpu;

pub struct SdrPassData {
    pub src_image: GfxImageViewHandle,
    pub src_image_size: vk::Extent2D,

    pub dst_image: GfxImageViewHandle,
    pub dst_image_size: vk::Extent2D,
}

pub struct SdrPass {
    sdr_pass: ComputePass<gpu::sdr::PushConstant>,
}
impl SdrPass {
    pub fn new(render_descriptor_sets: &GlobalDescriptorSets) -> Self {
        let sdr_pass = ComputePass::<gpu::sdr::PushConstant>::new(
            render_descriptor_sets,
            c"main",
            TruvisPath::shader_build_path_str("pp/sdr.slang").as_str(),
        );

        Self { sdr_pass }
    }

    pub fn exec(&self, cmd: &GfxCommandBuffer, data: SdrPassData, render_world: &RenderWorld) {
        let src_image_bindless_handle = render_world.bindless_manager.get_shader_uav_handle(data.src_image);
        let dst_image_bindless_handle = render_world.bindless_manager.get_shader_uav_handle(data.dst_image);

        let frame_label = render_world.frame_counter.frame_label();
        self.sdr_pass.exec(
            cmd,
            frame_label,
            &render_world.global_descriptor_sets,
            &gpu::sdr::PushConstant {
                src_image: src_image_bindless_handle.0,
                dst_image: dst_image_bindless_handle.0,
                image_size: glam::uvec2(data.src_image_size.width, data.src_image_size.height).into(),
                channel: render_world.pipeline_settings.channel,
                _padding_1: Default::default(),
            },
            glam::uvec3(
                data.dst_image_size.width.div_ceil(gpu::blit::SHADER_X as u32),
                data.dst_image_size.height.div_ceil(gpu::blit::SHADER_Y as u32),
                1,
            ),
        );
    }
}

pub struct SdrRgPass<'a> {
    pub sdr_pass: &'a SdrPass,

    // TODO 暂时使用这个肮脏的实现
    pub render_world: &'a RenderWorld,

    pub src_image: RgImageHandle,
    pub dst_image: RgImageHandle,

    pub src_image_extent: vk::Extent2D,
    pub dst_image_extent: vk::Extent2D,
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
            },
            self.render_world,
        );
    }
}
