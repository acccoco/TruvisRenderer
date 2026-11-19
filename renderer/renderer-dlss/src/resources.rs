use ash::vk;
use renderer_kit::targets::{ImageTarget, PerFrameImageSet, TargetImageDesc};
use slotmap::Key;
use truvis_gfx::{
    gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx},
    resources::{image_view::GfxImageViewDesc, lifecycle::DestroyReason},
};
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_render_runtime::{
    render_runtime_ctx::{RenderRuntimeInitCtx, RenderRuntimeResizeCtx, RenderRuntimeShutdownCtx},
    resources::gfx_resource_registry::GfxResourceRegistry,
    state::frame_state::FrameRenderState,
};

/// SR/RR 共用按 FIF 轮转的 HDR 输出。SR 固定曝光图像由所有帧只读共享，resize 时不重建。
pub(super) struct DlssTargets
{
    output: PerFrameImageSet,
    exposure: ImageTarget,
}

impl DlssTargets
{
    pub(super) fn new(ctx: &mut RenderRuntimeInitCtx<'_>) -> Self
    {
        let frame_id = ctx.frame_timing.frame_id();
        let desc = TargetImageDesc {
            name_prefix: "dlss-sr-exposure",
            format: vk::Format::R32_SFLOAT,
            extent: vk::Extent2D { width: 1, height: 1 },
            usage: vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
        };
        let name = format!("{}-{frame_id}", desc.name_prefix);
        let image = desc.create_image(ctx.resource_ctx, &name);

        // 固定曝光图像上传后保持 shader read layout，graph 不得丢弃其内容。
        let bytes = 1.0_f32.to_ne_bytes();
        let stage_buffer = ctx
            .immediate_ctx
            .one_time_exec(|cmd| image.transfer_data(ctx.resource_ctx, cmd, &bytes), "dlss-sr-exposure-upload");
        stage_buffer.destroy(ctx.resource_ctx, DestroyReason::ScopeDrop);
        let image = ctx.gfx_resource_registry.register_image(image);
        let view = ctx.gfx_resource_registry.get_or_create_image_view(
            ctx.device_ctx,
            image,
            GfxImageViewDesc::new_2d(desc.format, vk::ImageAspectFlags::COLOR),
            name,
        );
        let exposure = ImageTarget {
            image,
            view,
            format: desc.format,
            extent: desc.extent,
        };
        let output = Self::create_output(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            ctx.gfx_resource_registry,
            ctx.frame_state,
            frame_id,
        );
        Self { output, exposure }
    }

    pub(super) fn resize(&mut self, ctx: &mut RenderRuntimeResizeCtx<'_>)
    {
        self.output.destroy(ctx.resource_ctx, ctx.device_ctx, ctx.gfx_resource_registry, DestroyReason::Resize);
        self.output = Self::create_output(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            ctx.gfx_resource_registry,
            ctx.frame_state,
            ctx.frame_timing.frame_id(),
        );
    }

    pub(super) fn destroy(mut self, ctx: &mut RenderRuntimeShutdownCtx<'_>)
    {
        let exposure = std::mem::take(&mut self.exposure);
        ctx.gfx_resource_registry.release_image_immediate(
            ctx.resource_ctx,
            ctx.device_ctx,
            exposure.image,
            DestroyReason::Shutdown,
        );
        self.output.destroy(ctx.resource_ctx, ctx.device_ctx, ctx.gfx_resource_registry, DestroyReason::Shutdown);
    }

    pub(super) fn output(&self, frame_label: FrameLabel) -> ImageTarget
    {
        self.output.target(frame_label)
    }

    pub(super) fn exposure(&self) -> ImageTarget
    {
        self.exposure
    }

    fn create_output(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        registry: &mut GfxResourceRegistry,
        frame_state: &FrameRenderState,
        frame_id: u64,
    ) -> PerFrameImageSet
    {
        // SDR 以 storage image 读取输出，debug preview 对同一图像采样。
        PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            registry,
            TargetImageDesc {
                name_prefix: "dlss-output",
                format: frame_state.hdr_color_format,
                extent: frame_state.output_extent,
                usage: vk::ImageUsageFlags::STORAGE |
                    vk::ImageUsageFlags::TRANSFER_SRC |
                    vk::ImageUsageFlags::TRANSFER_DST |
                    vk::ImageUsageFlags::SAMPLED,
            },
            frame_id,
        )
    }
}

impl Drop for DlssTargets
{
    fn drop(&mut self)
    {
        debug_assert!(self.exposure.image.is_null());
        debug_assert!(self.exposure.view.is_null());
    }
}
