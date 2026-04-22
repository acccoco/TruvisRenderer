use ash::vk;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_path::TruvisPath;
use truvis_render_graph::compute_pass::ComputePass;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_interface::bindless_manager::BindlessUavHandle;
use truvis_render_interface::global_descriptor_sets::GlobalDescriptorSets;
use truvis_renderer::render_context::RenderContext;
use truvis_shader_binding::gpu;

pub struct BlitPassData {
    pub src_bindless_uav_handle: BindlessUavHandle,
    pub dst_bindless_uav_handle: BindlessUavHandle,

    pub src_image_size: vk::Extent2D,
    pub dst_image_size: vk::Extent2D,
}

pub struct BlitPass {
    blit_pass: ComputePass<gpu::blit::PushConstant>,
}
impl BlitPass {
    pub fn new(render_descriptor_sets: &GlobalDescriptorSets) -> Self {
        let blit_pass = ComputePass::<gpu::blit::PushConstant>::new(
            render_descriptor_sets,
            c"main",
            TruvisPath::shader_build_path_str("imgui/blit.slang").as_str(),
        );

        Self { blit_pass }
    }

    pub fn exec(&self, cmd: &GfxCommandBuffer, data: BlitPassData, render_context: &RenderContext) {
        let frame_label = render_context.frame_counter.frame_label();
        self.blit_pass.exec(
            cmd,
            frame_label,
            &render_context.global_descriptor_sets,
            &gpu::blit::PushConstant {
                src_image: data.src_bindless_uav_handle.0,
                dst_image: data.dst_bindless_uav_handle.0,
                src_image_size: glam::uvec2(data.src_image_size.width, data.dst_image_size.height).into(),
                offset: glam::uvec2(0, 0).into(),
            },
            glam::uvec3(
                data.dst_image_size.width.div_ceil(gpu::blit::SHADER_X as u32),
                data.dst_image_size.height.div_ceil(gpu::blit::SHADER_Y as u32),
                1,
            ),
        );
    }
}

pub struct BlitRgPass<'a> {
    pub blit_pass: &'a BlitPass,

    // TODO 暂时使用这个肮脏的实现
    pub render_context: &'a RenderContext,

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
        let src_bindless_uav_handle = self.render_context.bindless_manager.get_shader_uav_handle(src_image_handle);
        let dst_bindless_uav_handle = self.render_context.bindless_manager.get_shader_uav_handle(dst_image_handle);

        self.blit_pass.exec(
            ctx.cmd,
            BlitPassData {
                src_bindless_uav_handle,
                dst_bindless_uav_handle,
                src_image_size: self.src_image_extent,
                dst_image_size: self.dst_image_extent,
            },
            self.render_context,
        );
    }
}
