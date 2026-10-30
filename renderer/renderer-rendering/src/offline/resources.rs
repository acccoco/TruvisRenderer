//! Offline 子系统独占的累计图像与 per-frame 输出目标。

use ash::vk;
use slotmap::Key;

use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_render_runtime::resources::gfx_resource_registry::GfxResourceRegistry;
use truvis_render_runtime::state::frame_state::FrameRenderState;

use crate::shared::targets::{ImageTarget, PerFrameImageSet, SingleImageTarget, TargetImageDesc};

/// 离线 ground truth 渲染子系统的窗口尺寸图像。
///
/// `accum_image` 是唯一跨帧历史，不能按 frame label 轮转；`single_frame_image` 和
/// `render_target` 仍是 per-FIF 图像，分别服务当前采样输出和 present graph 输入。
pub struct OfflineTargets {
    single_frame_image: PerFrameImageSet,
    accum_image: SingleImageTarget,
    render_target: PerFrameImageSet,
}

impl OfflineTargets {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_registry: &mut GfxResourceRegistry,
        frame_state: &FrameRenderState,
        frame_id: u64,
    ) -> Self {
        // 中间图像由 RT/Accum/SDR 通过 storage 访问、由 Resolve/debug viewer 采样；attachment 位于 swapchain。
        let storage_usage =
            vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::SAMPLED;

        let single_frame_image = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_registry,
            TargetImageDesc {
                name_prefix: "offline-single-frame",
                format: frame_state.hdr_color_format,
                extent: frame_state.render_extent,
                usage: storage_usage,
            },
            frame_id,
        );
        let accum_image = SingleImageTarget::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_registry,
            TargetImageDesc {
                name_prefix: "offline-accum",
                format: frame_state.hdr_color_format,
                extent: frame_state.render_extent,
                usage: storage_usage,
            },
            frame_id,
        );
        let render_target = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_registry,
            TargetImageDesc {
                name_prefix: "offline-render-target",
                format: frame_state.hdr_color_format,
                extent: frame_state.output_extent,
                usage: storage_usage,
            },
            frame_id,
        );

        Self {
            single_frame_image,
            accum_image,
            render_target,
        }
    }

    pub fn rebuild(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_registry: &mut GfxResourceRegistry,
        frame_state: &FrameRenderState,
        frame_id: u64,
    ) {
        self.destroy(resource_ctx, device_ctx, gfx_resource_registry, DestroyReason::Resize);
        *self = Self::new(resource_ctx, device_ctx, immediate_ctx, gfx_resource_registry, frame_state, frame_id);
    }

    pub fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_registry: &mut GfxResourceRegistry,
        reason: DestroyReason,
    ) {
        // 三组都已越过 GPU safe point；完成本 owner 的清理后再传播首个释放诊断。
        self.single_frame_image.destroy(resource_ctx, device_ctx, gfx_resource_registry, reason);
        self.accum_image.destroy(resource_ctx, device_ctx, gfx_resource_registry, reason);
        self.render_target.destroy(resource_ctx, device_ctx, gfx_resource_registry, reason);
    }

    #[inline]
    pub fn single_frame_image(&self, frame_label: FrameLabel) -> ImageTarget {
        self.single_frame_image.target(frame_label)
    }

    #[inline]
    pub fn accum_image(&self) -> ImageTarget {
        self.accum_image.target()
    }

    #[inline]
    pub fn render_target(&self, frame_label: FrameLabel) -> ImageTarget {
        self.render_target.target(frame_label)
    }
}

impl Drop for OfflineTargets {
    fn drop(&mut self) {
        debug_assert!(self.single_frame_image.images.iter().all(|img| img.is_null()));
        debug_assert!(self.accum_image.image.is_null());
        debug_assert!(self.render_target.images.iter().all(|img| img.is_null()));
    }
}
