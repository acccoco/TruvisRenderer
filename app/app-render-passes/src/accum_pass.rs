use ash::vk;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_path::TruvisPath;
use truvis_render_foundation::bindless_manager::BindlessUavHandle;
use truvis_render_foundation::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_graph::compute_pass::ComputePass;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;
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
    pub fn new(ctx: GfxDeviceCtx<'_>, render_descriptor_sets: &GlobalDescriptorSets) -> Self {
        let accum_pass = ComputePass::<gpu::accum::PushConstant>::new(
            ctx,
            render_descriptor_sets,
            c"main",
            TruvisPath::shader_build_path_str("pp/accum.slang").as_str(),
        );

        Self { accum_pass }
    }

    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        self.accum_pass.destroy(ctx);
    }

    pub fn exec(&self, cmd: &GfxCommandBuffer, data: AccumPassData, record_ctx: &RenderPassRecordCtx<'_>) {
        let frame_label = record_ctx.frame_timing.frame_label();
        self.accum_pass.exec(
            cmd,
            frame_label,
            record_ctx.shader_bindings.global_descriptor_sets(),
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

    pub record_ctx: RenderPassRecordCtx<'a>,

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
            self.record_ctx.shader_bindings.get_shader_uav_handle(single_frame_view_handle);
        let accum_bindless_uav_handle = self.record_ctx.shader_bindings.get_shader_uav_handle(accum_view_handle);

        self.accum_pass.exec(
            ctx.cmd,
            AccumPassData {
                single_frame_bindless_uav_handle,
                accum_bindless_uav_handle,
                image_size: self.image_extent,
                accum_frames: self.record_ctx.view_accum.accum_frames_num() as u32,
            },
            &self.record_ctx,
        );
    }
}
