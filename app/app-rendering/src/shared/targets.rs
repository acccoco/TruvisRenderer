//! Realtime/offline 子系统共享的 image target 构建和资源释放契约。
//!
//! 这些资源描述具体渲染子系统需要的中间图像，而不是 engine 的帧调度基础设施。
//! owner 只保存 `GfxResourceManager` handle；创建、resize 和 shutdown 时由
//! `RealtimeRenderSubsystem` 通过生命周期 ctx 显式传入 manager 与 typed Gfx ctx。
//!
//! 设计边界：
//! - `FrameLabel` 仍来自 engine，用来表达当前使用哪个在飞帧槽位。
//! - 具体图像的用途、格式和 resize 生命周期属于 app 层渲染策略；shader 可见性由消费它的 pass-local descriptor 表达。
//! - 本模块不保存 `Gfx` / device / allocator 引用，避免长期资源 owner 反向持有 runtime 能力。

use ash::vk;
use itertools::Itertools;
use slotmap::Key;
use truvis_gfx::commands::barrier::GfxImageBarrier;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::resources::image::{GfxImage, GfxImageCreateInfo};
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_foundation::frame_label::FrameLabel;
use truvis_render_foundation::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_render_runtime::resources::gfx_resource_manager::GfxResourceManager;

/// RenderGraph 导入图像所需的 handle、格式和尺寸快照。
///
/// 这里的 handle 不是资源所有权本身，而是 `GfxResourceManager` 中已注册对象的稳定索引。
/// 调用方只能在 owner 存活期间把它导入 RenderGraph；真实释放仍由 owner 在
/// resize / shutdown 阶段通过 manager 显式完成。
#[derive(Clone, Copy)]
pub struct ImageTarget {
    /// manager-owned image handle，用于 RenderGraph import。
    pub image: GfxImageHandle,
    /// 对应 image view handle；具体 pass 在录制时把它写入自己的 descriptor。
    pub view: GfxImageViewHandle,
    /// pass 创建和 RenderGraph import 必须使用同一格式，避免 view 与 pipeline attachment 不一致。
    pub format: vk::Format,
    /// target 创建时的窗口尺寸快照，供 pass 设置 viewport、dispatch size 或 copy/resolve extent。
    pub extent: vk::Extent2D,
}

/// 一组按 frame label 轮转的窗口尺寸图像。
///
/// 这类 target 会被当前 frame label 对应的 command buffer 写入；同一 label 再次复用前，
/// runtime 的 FIF timeline 已经保证上一轮提交完成。因此它适合放置单帧 RT 输出、
/// main view color 等“每个在飞帧各一份”的图像。
pub(crate) struct PerFrameImageSet {
    pub(crate) images: [GfxImageHandle; FrameLabel::COUNT],
    views: [GfxImageViewHandle; FrameLabel::COUNT],
    pub(crate) format: vk::Format,
    pub(crate) extent: vk::Extent2D,
}

impl PerFrameImageSet {
    pub(crate) fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        desc: TargetImageDesc<'_>,
        frame_id: u64,
    ) -> Self {
        // image 先创建为未注册的 Vulkan wrapper，方便在同一批 immediate 命令中做初始 layout 转换；
        // 转换完成后再注册到 manager，后续只通过 handle 暴露给 RenderGraph 和具体 pass。
        let create_one_image = |frame_label: FrameLabel| {
            create_image(
                resource_ctx,
                desc.extent,
                desc.format,
                desc.usage,
                format!("{}-{}-{}", desc.name_prefix, frame_label, frame_id),
            )
        };
        let images = FrameLabel::ALL.map(create_one_image);

        transition_images_to_general(immediate_ctx, &images, &format!("transfer-{}-layout", desc.name_prefix));

        // view 生命周期由 `GfxResourceManager` 跟随 image 释放。owner 只保存 view handle；
        // pass-local descriptor 在 command recording 时按值写入，不形成额外的长期资源 owner。
        let image_handles = images.map(|image| gfx_resource_manager.register_image(image));
        let image_view_handles = FrameLabel::ALL.map(|frame_label| {
            gfx_resource_manager.get_or_create_image_view(
                device_ctx,
                image_handles[*frame_label],
                GfxImageViewDesc::new_2d(desc.format, vk::ImageAspectFlags::COLOR),
                format!("{}-{}-{}", desc.name_prefix, frame_label, frame_id),
            )
        });

        Self {
            images: image_handles,
            views: image_view_handles,
            format: desc.format,
            extent: desc.extent,
        }
    }

    pub(crate) fn target(&self, frame_label: FrameLabel) -> ImageTarget {
        ImageTarget {
            image: self.images[*frame_label],
            view: self.views[*frame_label],
            format: self.format,
            extent: self.extent,
        }
    }

    pub(crate) fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        // view 会由 manager 在释放 image 时按 image-view-before-image 顺序处理。调用方仍必须遵守
        // resize/shutdown 的 GPU safe point；pass-local descriptor 不改变底层 image 的在飞生命周期。
        for image in std::mem::take(&mut self.images) {
            gfx_resource_manager.release_image_immediate(resource_ctx, device_ctx, image, reason);
        }
        self.views = Default::default();
    }
}

impl Drop for PerFrameImageSet {
    fn drop(&mut self) {
        debug_assert!(self.images.iter().all(|img| img.is_null()));
    }
}
/// 单张窗口尺寸图像。
///
/// 这类 target 不随 frame label 轮转，适合保存跨帧持续累积的渲染子系统私有历史。
/// 调用方必须通过 RenderGraph 为每次读写声明状态；本类型只负责 image/view 生命周期，
/// 不表达 descriptor 绑定或任何跨帧同步语义。
pub(crate) struct SingleImageTarget {
    pub(crate) image: GfxImageHandle,
    view: GfxImageViewHandle,
    pub(crate) format: vk::Format,
    pub(crate) extent: vk::Extent2D,
}

impl SingleImageTarget {
    pub(crate) fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        desc: TargetImageDesc<'_>,
        frame_id: u64,
    ) -> Self {
        let image = create_image(
            resource_ctx,
            desc.extent,
            desc.format,
            desc.usage,
            format!("{}-{}", desc.name_prefix, frame_id),
        );
        transition_images_to_general(
            immediate_ctx,
            std::slice::from_ref(&image),
            &format!("transfer-{}-layout", desc.name_prefix),
        );

        let image_handle = gfx_resource_manager.register_image(image);
        let view_handle = gfx_resource_manager.get_or_create_image_view(
            device_ctx,
            image_handle,
            GfxImageViewDesc::new_2d(desc.format, vk::ImageAspectFlags::COLOR),
            format!("{}-{}", desc.name_prefix, frame_id),
        );

        Self {
            image: image_handle,
            view: view_handle,
            format: desc.format,
            extent: desc.extent,
        }
    }

    pub(crate) fn target(&self) -> ImageTarget {
        ImageTarget {
            image: self.image,
            view: self.view,
            format: self.format,
            extent: self.extent,
        }
    }

    pub(crate) fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        gfx_resource_manager.release_image_immediate(resource_ctx, device_ctx, self.image, reason);
        self.image = GfxImageHandle::default();
        self.view = GfxImageViewHandle::default();
    }
}

impl Drop for SingleImageTarget {
    fn drop(&mut self) {
        debug_assert!(self.image.is_null());
        debug_assert!(self.view.is_null());
    }
}

impl Default for ImageTarget {
    fn default() -> Self {
        Self {
            image: GfxImageHandle::default(),
            view: GfxImageViewHandle::default(),
            format: vk::Format::UNDEFINED,
            extent: vk::Extent2D::default(),
        }
    }
}

pub(crate) struct TargetImageDesc<'a> {
    /// 资源名用于 debug name、Tracy span 和 destroy 日志定位。
    pub(crate) name_prefix: &'a str,
    /// image 与 view 使用同一格式；调用方负责保证 pipeline attachment 格式匹配。
    pub(crate) format: vk::Format,
    /// 创建时的窗口尺寸快照，resize 后必须重建 owner。
    pub(crate) extent: vk::Extent2D,
    /// 由具体 target 语义决定的 Vulkan usage，不在 engine 中硬编码。
    pub(crate) usage: vk::ImageUsageFlags,
}

pub(crate) fn create_image(
    resource_ctx: GfxResourceCtx<'_>,
    extent: vk::Extent2D,
    format: vk::Format,
    usage: vk::ImageUsageFlags,
    name: impl AsRef<str>,
) -> GfxImage {
    let image_create_info = GfxImageCreateInfo::new_image_2d_info(extent, format, usage);
    GfxImage::new(
        resource_ctx,
        &image_create_info,
        &vk_mem::AllocationCreateInfo {
            usage: vk_mem::MemoryUsage::AutoPreferDevice,
            ..Default::default()
        },
        name.as_ref(),
    )
}

fn transition_images_to_general(immediate_ctx: GfxImmediateCtx<'_>, images: &[GfxImage], label: &str) {
    // storage target 在本项目里以 GENERAL 作为初始稳定布局，
    // 后续精确读写状态由 RenderGraph 在每帧导入后继续接管。
    immediate_ctx.one_time_exec(
        |cmd| {
            let image_barriers = images
                .iter()
                .map(|image| {
                    GfxImageBarrier::default()
                        .image(image.handle())
                        .src_mask(vk::PipelineStageFlags2::TOP_OF_PIPE, vk::AccessFlags2::empty())
                        .dst_mask(vk::PipelineStageFlags2::BOTTOM_OF_PIPE, vk::AccessFlags2::empty())
                        .layout_transfer(vk::ImageLayout::UNDEFINED, vk::ImageLayout::GENERAL)
                        .image_aspect_flag(vk::ImageAspectFlags::COLOR)
                })
                .collect_vec();

            cmd.image_memory_barrier(vk::DependencyFlags::empty(), &image_barriers);
        },
        label,
    );
}
