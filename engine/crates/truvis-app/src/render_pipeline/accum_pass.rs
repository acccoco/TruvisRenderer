use ash::vk;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_path::TruvisPath;
use truvis_render_graph::compute_pass::ComputePass;
use truvis_render_graph::render_context::RenderContext;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_interface::bindless_manager::BindlessUavHandle;
use truvis_render_interface::global_descriptor_sets::GlobalDescriptorSets;
use truvis_shader_binding::gpu;

/// 累积 Pass 的数据
pub struct AccumPassData {
    pub single_frame_bindless_uav_handle: BindlessUavHandle,
    pub accum_bindless_uav_handle: BindlessUavHandle,
    pub image_size: vk::Extent2D,
    pub accum_frames: u32,
}

/// 累积 Pass - 将单帧 RT 结果累积到 accum_image 中
pub struct AccumPass {
    accum_pass: ComputePass<gpu::accum::PushConstant>,
}

impl AccumPass {
    pub fn new(render_descriptor_sets: &GlobalDescriptorSets) -> Self {
        let accum_pass = ComputePass::<gpu::accum::PushConstant>::new(
            render_descriptor_sets,
            c"main",
            TruvisPath::shader_build_path_str("pp/accum.slang").as_str(),
        );

        Self { accum_pass }
    }

    pub fn exec(&self, cmd: &GfxCommandBuffer, data: AccumPassData, render_context: &RenderContext) {
        self.accum_pass.exec(
            cmd,
            render_context,
            &gpu::accum::PushConstant {
                single_frame_input: data.single_frame_bindless_uav_handle.0,
                accum_output: data.accum_bindless_uav_handle.0,
                image_size: glam::uvec2(data.image_size.width, data.image_size.height).into(),
                accum_frames: data.accum_frames,
                _padding_: 0,
            },
            glam::uvec3(
                data.image_size.width.div_ceil(gpu::accum::SHADER_X as u32),
                data.image_size.height.div_ceil(gpu::accum::SHADER_Y as u32),
                1,
            ),
        );
    }
}

/// 累积 Pass 的 RenderGraph 封装
pub struct AccumRgPass<'a> {
    pub accum_pass: &'a AccumPass,

    // TODO 暂时使用这个肮脏的实现
    pub render_context: &'a RenderContext,

    /// 单帧 RT 输出（只读）
    pub single_frame_image: RgImageHandle,
    /// 累积结果（读写）
    pub accum_image: RgImageHandle,

    pub image_extent: vk::Extent2D,
}

impl<'a> RgPass for AccumRgPass<'a> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        // 读取单帧 RT 输出
        builder.read_image(self.single_frame_image, RgImageState::STORAGE_READ_COMPUTE);
        // 读写累积图像
        builder.read_write_image(self.accum_image, RgImageState::STORAGE_READ_WRITE_COMPUTE);
    }

    fn execute(&self, ctx: &RgPassContext) {
        let single_frame_view_handle = ctx.get_image_view_handle(self.single_frame_image).unwrap();
        let accum_view_handle = ctx.get_image_view_handle(self.accum_image).unwrap();

        let single_frame_bindless_uav_handle =
            self.render_context.bindless_manager.get_shader_uav_handle(single_frame_view_handle);
        let accum_bindless_uav_handle = self.render_context.bindless_manager.get_shader_uav_handle(accum_view_handle);

        self.accum_pass.exec(
            ctx.cmd,
            AccumPassData {
                single_frame_bindless_uav_handle,
                accum_bindless_uav_handle,
                image_size: self.image_extent,
                accum_frames: self.render_context.accum_data.accum_frames_num() as u32,
            },
            self.render_context,
        );
    }
}
