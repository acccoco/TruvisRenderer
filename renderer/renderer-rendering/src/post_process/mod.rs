use ash::vk;

use renderer_kit::subsystem::{SubsystemLifecycle, SubsystemRenderCtx};
use renderer_render_passes::post_process::auto_exposure::{AutoExposurePass, AutoExposureRgPass, ExposureStage};
use renderer_render_passes::post_process::image_clear::{ImageClearPass, ImageClearRgPass};
use renderer_render_passes::post_process::sdr::{SdrPass, SdrRgPass};
use truvis_gfx::commands::barrier::GfxImageBarrier;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle, RgImageState};
use truvis_render_runtime::render_runtime::{RenderRuntimeInitCtx, RenderRuntimeResizeCtx, RenderRuntimeShutdownCtx};
use truvis_renderer_shader_binding::gpu;

use crate::shared::settings::{ExposureMode, SdrPostProcessSettings};
use crate::shared::targets::{SingleImageTarget, TargetImageDesc};
use crate::shared::{PathTracingDebugChannel, RenderMode};

/// 当前 graph 中的借用视图，不转移 realtime/offline target 所有权。
pub struct SdrPostProcessInput {
    pub source: RgImageHandle,
    pub destination: RgImageHandle,
    pub source_extent: vk::Extent2D,
    pub destination_extent: vk::Extent2D,
    pub channel: PathTracingDebugChannel,
}

/// 主视图唯一的显示后处理 owner。曝光按实际提交顺序推进，不随 FIF 轮转。
#[derive(Default)]
pub struct SdrPostProcess {
    resources: Option<SdrResources>,
    initialized: bool,
    last_mode: Option<(RenderMode, ExposureMode)>,
    pending_snap: bool,
}

struct SdrResources {
    sdr: SdrPass,
    meter: AutoExposurePass,
    clear: ImageClearPass,
    histogram: SingleImageTarget,
    exposure: SingleImageTarget,
    lut: SingleImageTarget,
}

impl SdrResources {
    fn target(
        ctx: &mut RenderRuntimeInitCtx<'_>,
        name: &str,
        extent: vk::Extent2D,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
    ) -> SingleImageTarget {
        SingleImageTarget::new(
            ctx.resource_ctx,
            ctx.device_ctx,
            ctx.immediate_ctx,
            &mut *ctx.gfx_resource_registry,
            TargetImageDesc {
                name_prefix: name,
                extent,
                format,
                usage,
            },
            ctx.frame_timing.frame_id(),
        )
    }

    fn new(ctx: &mut RenderRuntimeInitCtx<'_>) -> Self {
        let histogram = Self::target(
            ctx,
            "exposure-histogram",
            vk::Extent2D { width: 256, height: 1 },
            vk::Format::R32_UINT,
            vk::ImageUsageFlags::STORAGE,
        );
        let exposure = Self::target(
            ctx,
            "exposure-history",
            vk::Extent2D { width: 1, height: 1 },
            vk::Format::R32G32B32A32_SFLOAT,
            vk::ImageUsageFlags::STORAGE,
        );
        let lut = Self::target(
            ctx,
            "agx-default-lut",
            vk::Extent2D {
                width: 1024,
                height: 32,
            },
            vk::Format::R16G16B16A16_SFLOAT,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
        );
        let image = ctx.gfx_resource_registry.get_image(lut.image).unwrap();
        let staging = ctx.immediate_ctx.one_time_exec(
            |cmd| {
                let staging = image.transfer_data(ctx.resource_ctx, cmd, include_bytes!("../../resources/agx-default.rgba16f"));
                // 通用上传 helper 的读取终点是 fragment；LUT 实际由 compute 读取，显式补全可见性。
                let barrier = GfxImageBarrier::new()
                    .image(image.handle())
                    .src_mask(vk::PipelineStageFlags2::TRANSFER, vk::AccessFlags2::TRANSFER_WRITE)
                    .dst_mask(vk::PipelineStageFlags2::COMPUTE_SHADER, vk::AccessFlags2::SHADER_SAMPLED_READ)
                    .layout_transfer(
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    )
                    .image_aspect_flag(vk::ImageAspectFlags::COLOR);
                cmd.image_memory_barrier(vk::DependencyFlags::empty(), std::slice::from_ref(&barrier));
                staging
            },
            "upload-agx-lut",
        );
        staging.destroy(ctx.resource_ctx, DestroyReason::ScopeDrop);
        Self {
            sdr: SdrPass::new(ctx.device_ctx, ctx.shader_binding_system.global_descriptor_sets()),
            meter: AutoExposurePass::new(ctx.device_ctx, ctx.shader_binding_system.global_descriptor_sets()),
            clear: ImageClearPass::new(ctx.device_ctx, ctx.shader_binding_system.global_descriptor_sets()),
            histogram,
            exposure,
            lut,
        }
    }

    fn destroy(mut self, ctx: &mut RenderRuntimeShutdownCtx<'_>) {
        self.sdr.destroy(ctx.device_ctx);
        self.meter.destroy(ctx.device_ctx);
        self.clear.destroy(ctx.device_ctx);
        for target in [&mut self.histogram, &mut self.exposure, &mut self.lut] {
            target.destroy(ctx.resource_ctx, ctx.device_ctx, &mut *ctx.gfx_resource_registry, DestroyReason::Shutdown);
        }
    }
}

impl SubsystemLifecycle for SdrPostProcess {
    fn init(&mut self, ctx: &mut RenderRuntimeInitCtx<'_>) {
        self.resources = Some(SdrResources::new(ctx));
        self.initialized = false;
        self.last_mode = None;
        self.pending_snap = true;
    }
    fn on_resize(&mut self, _ctx: &mut RenderRuntimeResizeCtx<'_>) {
        // 固定大小资源及曝光历史跨 resize 保留；输入尺寸由当前 graph 提供。
    }
    fn shutdown(&mut self, ctx: &mut RenderRuntimeShutdownCtx<'_>) {
        if let Some(resources) = self.resources.take() {
            resources.destroy(ctx);
        }
    }
}

impl SdrPostProcess {
    pub fn contribute<'a>(
        &'a mut self,
        graph: &mut RenderGraphBuilder<'a>,
        ctx: &'a SubsystemRenderCtx<'a>,
        input: Option<SdrPostProcessInput>,
        mode: RenderMode,
        settings: SdrPostProcessSettings,
    ) {
        let mode_key = (mode, settings.exposure.mode);
        if self.last_mode != Some(mode_key) {
            self.pending_snap = true;
            self.last_mode = Some(mode_key);
        }
        let Some(input) = input else {
            return;
        };
        let resources = self.resources.as_ref().expect("SDR resources initialized");
        let exposure_target = resources.exposure.target();
        let exposure = graph.import_image(
            "exposure-history",
            exposure_target.image,
            Some(exposure_target.view),
            exposure_target.format,
            if self.initialized { RgImageState::STORAGE_READ_WRITE_COMPUTE } else { RgImageState::UNDEFINED_TOP },
            None,
        );
        if !self.initialized {
            graph.add_pass(
                "exposure-initialize",
                ImageClearRgPass {
                    clear_pass: &resources.clear,
                    record_ctx: ctx.record_ctx,
                    dst_image: exposure,
                    image_extent: exposure_target.extent,
                    clear_color: glam::Vec4::ZERO,
                },
            );
            self.initialized = true;
        }
        let metering = settings.exposure.mode == ExposureMode::Auto
            && !settings.exposure.locked
            && input.channel == PathTracingDebugChannel::Final;
        if metering {
            let target = resources.histogram.target();
            // 上一帧归约读取到本帧清零，以及后续原子读写，均由同队列 image barrier 串行化。
            let histogram = graph.import_image(
                "exposure-histogram",
                target.image,
                Some(target.view),
                target.format,
                RgImageState::STORAGE_READ_COMPUTE,
                None,
            );
            for (stage, name) in [
                (ExposureStage::Clear, "exposure-clear"),
                (ExposureStage::Histogram, "exposure-histogram"),
                (ExposureStage::Reduce, "exposure-reduce"),
            ] {
                let params = gpu::renderer::render_passes::auto_exposure::PushConstant {
                    image_size: glam::uvec2(input.source_extent.width, input.source_extent.height).into(),
                    metering_mode: settings.exposure.metering as u32,
                    instant: (self.pending_snap || mode == RenderMode::Offline) as u32,
                    low_percentile: settings.exposure.low_percentile,
                    high_percentile: settings.exposure.high_percentile,
                    min_ev: settings.exposure.min_ev,
                    max_ev: settings.exposure.max_ev,
                    delta_seconds: ctx.record_ctx.frame_timing.delta_time_s(),
                    darken_seconds: settings.exposure.darken_seconds,
                    brighten_seconds: settings.exposure.brighten_seconds,
                    _padding_0: 0,
                };
                graph.add_pass(
                    name,
                    AutoExposureRgPass {
                        pass: &resources.meter,
                        stage,
                        record_ctx: ctx.record_ctx,
                        source: input.source,
                        histogram,
                        exposure,
                        params,
                    },
                );
            }
            self.pending_snap = false;
        }
        let lut_target = resources.lut.target();
        let lut = graph.import_image(
            "agx-lut",
            lut_target.image,
            Some(lut_target.view),
            lut_target.format,
            RgImageState::SHADER_READ_COMPUTE,
            None,
        );
        graph.add_pass(
            "hdr-to-sdr",
            SdrRgPass {
                sdr_pass: &resources.sdr,
                record_ctx: ctx.record_ctx,
                src_image: input.source,
                dst_image: input.destination,
                src_image_extent: input.source_extent,
                dst_image_extent: input.destination_extent,
                radiance_channel: input.channel.is_radiance(),
                post_process: settings,
                exposure,
                agx_lut: lut,
            },
        );
        // 跨帧导入/导出保持相同状态，覆盖上一帧 reduce 写和 SDR 读，不依赖 FIF slot 等待。
        graph.export_image(exposure, RgImageState::STORAGE_READ_WRITE_COMPUTE, None);
    }
}
