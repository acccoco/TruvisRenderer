use ash::vk;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::GfxDeviceCtx;
use truvis_path::TruvisPath;
use truvis_render_foundation::handles::GfxImageViewHandle;
use truvis_render_graph::render_graph::{RgImageHandle, RgImageState, RgPass, RgPassBuilder, RgPassContext};
use truvis_render_runtime::bindings::global_descriptor_sets::GlobalDescriptorSets;
use truvis_render_runtime::render_runtime_ctx::RenderPassRecordCtx;
use truvis_shader_binding::gpu;

use crate::compute_pass::ComputePass;

/// 清理单张 storage image 所需的 CPU 侧参数。
///
/// `dst_image` 必须已经在 bindless 系统中注册为 UAV；`image_extent` 是本次 dispatch 的有效写入区域，
/// 不隐含完整 allocation 尺寸。越界线程由 shader 侧保护。
pub struct ImageClearPassData {
    pub dst_image: GfxImageViewHandle,
    pub image_extent: vk::Extent2D,
    pub clear_color: glam::Vec4,
}

/// 单张 storage image 的确定颜色清理 pass。
///
/// 该 pass 只封装 bindless UAV 与 compute dispatch 细节；图像所有权、layout transition
/// 和跨 pass 同步仍由调用方通过 RenderGraph 声明。
pub struct ImageClearPass {
    clear_pass: ComputePass<gpu::image_clear::PushConstant>,
}

impl ImageClearPass {
    pub fn new(ctx: GfxDeviceCtx<'_>, render_descriptor_sets: &GlobalDescriptorSets) -> Self {
        let clear_pass = ComputePass::<gpu::image_clear::PushConstant>::new(
            ctx,
            render_descriptor_sets,
            c"main",
            TruvisPath::shader_build_path_str("post/image_clear.slang").as_str(),
        );

        Self { clear_pass }
    }

    pub fn destroy(self, ctx: GfxDeviceCtx<'_>) {
        self.clear_pass.destroy(ctx);
    }

    pub fn exec(&self, cmd: &GfxCommandBuffer, data: ImageClearPassData, record_ctx: &RenderPassRecordCtx<'_>) {
        let dst_image_bindless_handle = record_ctx.shader_bindings.get_shader_uav_handle(data.dst_image);
        let frame_label = record_ctx.frame_timing.frame_label();
        self.clear_pass.exec(
            cmd,
            frame_label,
            record_ctx.shader_bindings.global_descriptor_sets(),
            &gpu::image_clear::PushConstant {
                clear_color: data.clear_color.into(),
                dst_image: dst_image_bindless_handle.0,
                _padding_0: 0,
                image_size: glam::uvec2(data.image_extent.width, data.image_extent.height).into(),
            },
            glam::uvec3(
                data.image_extent.width.div_ceil(gpu::image_clear::SHADER_X as u32),
                data.image_extent.height.div_ceil(gpu::image_clear::SHADER_Y as u32),
                1,
            ),
        );
    }
}

/// RenderGraph 适配层：声明一个只写 storage image 的 compute pass。
///
/// 本 pass 不读取历史内容；调用方如果希望丢弃旧图像，应在 import 时给出 `UNDEFINED_TOP`。
/// layout transition、UAV hazard 和跨 pass 同步全部由 RenderGraph 根据 `write_image` 声明生成。
pub struct ImageClearRgPass<'a> {
    pub clear_pass: &'a ImageClearPass,
    pub record_ctx: RenderPassRecordCtx<'a>,
    pub dst_image: RgImageHandle,
    pub image_extent: vk::Extent2D,
    pub clear_color: glam::Vec4,
}

impl RgPass for ImageClearRgPass<'_> {
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        builder.write_image(self.dst_image, RgImageState::STORAGE_WRITE_COMPUTE);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        let dst_image = ctx.get_image_view_handle(self.dst_image).expect("ImageClearRgPass: dst_image not found");
        self.clear_pass.exec(
            ctx.cmd,
            ImageClearPassData {
                dst_image,
                image_extent: self.image_extent,
                clear_color: self.clear_color,
            },
            &self.record_ctx,
        );
    }
}
