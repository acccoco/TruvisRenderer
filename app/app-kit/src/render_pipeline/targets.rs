//! App 层窗口尺寸渲染目标。
//!
//! 这些资源描述具体管线需要的中间图像，而不是 engine 的帧调度基础设施。
//! owner 只保存 `GfxResourceManager` handle；创建、resize 和 shutdown 时由
//! `RtPipeline` 通过生命周期 ctx 显式传入 manager 与 typed Gfx ctx。
//!
//! 设计边界：
//! - `FrameCounter` / `FrameLabel` 仍来自 engine，用来表达当前使用哪个在飞帧槽位。
//! - 具体图像的用途、格式、bindless 可见性和 resize 生命周期属于 app 层管线策略。
//! - 本模块不保存 `Gfx` / device / allocator 引用，避免长期资源 owner 反向持有 runtime 能力。

use ash::vk;
use itertools::Itertools;
use slotmap::Key;
use truvis_gfx::commands::barrier::GfxImageBarrier;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::resources::image::{GfxImage, GfxImageCreateInfo};
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_foundation::bindless_manager::BindlessManager;
use truvis_render_foundation::frame_counter::FrameCounter;
use truvis_render_foundation::gfx_resource_manager::GfxResourceManager;
use truvis_render_foundation::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_render_foundation::pipeline_settings::{FrameLabel, FrameSettings};

/// RenderGraph 导入图像所需的 handle、格式和尺寸快照。
///
/// 这里的 handle 不是资源所有权本身，而是 `GfxResourceManager` 中已注册对象的稳定索引。
/// 调用方只能在 owner 存活期间把它导入 RenderGraph；真实释放仍由 owner 在
/// resize / shutdown 阶段通过 manager 显式完成。
#[derive(Clone, Copy)]
pub struct ImageTarget {
    /// manager-owned image handle，用于 RenderGraph import。
    pub image: GfxImageHandle,
    /// 对应 image view handle；bindless 注册也以 view 为单位。
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
struct PerFrameImageSet {
    images: [GfxImageHandle; FrameCounter::fif_count()],
    views: [GfxImageViewHandle; FrameCounter::fif_count()],
    format: vk::Format,
    extent: vk::Extent2D,
}

impl PerFrameImageSet {
    fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        desc: TargetImageDesc<'_>,
        frame_counter: &FrameCounter,
    ) -> Self {
        // image 先创建为未注册的 Vulkan wrapper，方便在同一批 immediate 命令中做初始 layout 转换；
        // 转换完成后再注册到 manager，后续只通过 handle 暴露给 RenderGraph 和 bindless。
        let create_one_image = |frame_label: FrameLabel| {
            create_image(
                resource_ctx,
                desc.extent,
                desc.format,
                desc.usage,
                format!("{}-{}-{}", desc.name_prefix, frame_label, frame_counter.frame_id()),
            )
        };
        let images = FrameCounter::frame_labes().map(create_one_image);

        transition_images_to_general(immediate_ctx, &images, &format!("transfer-{}-layout", desc.name_prefix));

        // view 生命周期由 `GfxResourceManager` 跟随 image 释放。owner 只需要保存 view handle，
        // 并在销毁 image 前把 shader-visible bindless 注册撤掉。
        let image_handles = images.map(|image| gfx_resource_manager.register_image(image));
        let image_view_handles = FrameCounter::frame_labes().map(|frame_label| {
            gfx_resource_manager.get_or_create_image_view(
                device_ctx,
                image_handles[*frame_label],
                GfxImageViewDesc::new_2d(desc.format, vk::ImageAspectFlags::COLOR),
                format!("{}-{}-{}", desc.name_prefix, frame_label, frame_counter.frame_id()),
            )
        });

        Self {
            images: image_handles,
            views: image_view_handles,
            format: desc.format,
            extent: desc.extent,
        }
    }

    fn target(&self, frame_label: FrameLabel) -> ImageTarget {
        ImageTarget {
            image: self.images[*frame_label],
            view: self.views[*frame_label],
            format: self.format,
            extent: self.extent,
        }
    }

    fn register_uav(&self, bindless_manager: &mut BindlessManager) {
        for view in &self.views {
            bindless_manager.register_uav(*view);
        }
    }

    fn register_srv(&self, bindless_manager: &mut BindlessManager) {
        for view in &self.views {
            bindless_manager.register_srv(*view);
        }
    }

    fn unregister_uav(&self, bindless_manager: &mut BindlessManager) {
        for view in &self.views {
            bindless_manager.unregister_uav(*view);
        }
    }

    fn unregister_srv(&self, bindless_manager: &mut BindlessManager) {
        for view in &self.views {
            bindless_manager.unregister_srv(*view);
        }
    }

    fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        // 调用者必须先完成 bindless 注销；这里仅释放 manager-owned image。
        // view 会由 manager 在释放 image 时按 image-view-before-image 顺序处理。
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

/// RT 管线工作图像：单帧 ray tracing 输出和跨帧累积图像。
///
/// `single_frame_rt` 是 per-frame target，因为 raygen 每帧写入当前 FIF 槽位；
/// `accum` 是单张历史图像，因为 progressive accumulation 需要跨帧保留结果。
/// 当 resize 或环境/视图变化导致累积失效时，runtime 只重置 `AccumData`，
/// 图像本身由本 owner 在 resize 生命周期中重建。
pub struct RtWorkingTargets {
    single_frame_rt: PerFrameImageSet,
    accum: ImageTarget,
}

impl RtWorkingTargets {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        bindless_manager: &mut BindlessManager,
        frame_settings: &FrameSettings,
        frame_counter: &FrameCounter,
    ) -> Self {
        // RT working targets 既要作为 storage image 被 compute/ray tracing pass 写入，
        // 也可能被后续 pass 读取或 copy，因此统一保留 STORAGE / SAMPLED / TRANSFER_SRC 能力。
        let storage_usage =
            vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::SAMPLED;
        let single_frame_rt = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            TargetImageDesc {
                name_prefix: "single-frame-rt",
                format: frame_settings.color_format,
                extent: frame_settings.frame_extent,
                usage: storage_usage,
            },
            frame_counter,
        );
        let accum = create_single_color_target(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            TargetImageDesc {
                name_prefix: "accum-image",
                format: frame_settings.color_format,
                extent: frame_settings.frame_extent,
                usage: storage_usage,
            },
            frame_counter,
        );

        let targets = Self { single_frame_rt, accum };
        targets.register_bindless(bindless_manager);
        targets
    }

    pub fn rebuild(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        bindless_manager: &mut BindlessManager,
        gfx_resource_manager: &mut GfxResourceManager,
        frame_settings: &FrameSettings,
        frame_counter: &FrameCounter,
    ) {
        // resize 走 destroy + new，而不是在原 handle 上复用；这样旧尺寸 image/view/bindless slot
        // 会按明确的 DestroyReason 离开全局表，RenderGraph 下一帧只看到新尺寸 target。
        self.destroy(resource_ctx, device_ctx, bindless_manager, gfx_resource_manager, DestroyReason::Resize);
        *self = Self::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            bindless_manager,
            frame_settings,
            frame_counter,
        );
    }

    pub fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        bindless_manager: &mut BindlessManager,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        // bindless slot 可能仍被 shader-visible descriptor table 引用；必须先注销 view，
        // 再释放 manager image，避免后续 descriptor 更新读到已释放的 view handle。
        self.unregister_bindless(bindless_manager);
        self.single_frame_rt.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        gfx_resource_manager.release_image_immediate(resource_ctx, device_ctx, self.accum.image, reason);
        self.accum = ImageTarget::default();
    }

    #[inline]
    pub fn single_frame_rt(&self, frame_label: FrameLabel) -> ImageTarget {
        self.single_frame_rt.target(frame_label)
    }

    #[inline]
    pub fn accum(&self) -> ImageTarget {
        self.accum
    }

    fn register_bindless(&self, bindless_manager: &mut BindlessManager) {
        self.single_frame_rt.register_uav(bindless_manager);
        bindless_manager.register_uav(self.accum.view);
    }

    fn unregister_bindless(&self, bindless_manager: &mut BindlessManager) {
        self.single_frame_rt.unregister_uav(bindless_manager);
        bindless_manager.unregister_uav(self.accum.view);
    }
}

impl Drop for RtWorkingTargets {
    fn drop(&mut self) {
        debug_assert!(self.accum.image.is_null());
        debug_assert!(self.accum.view.is_null());
    }
}

/// 主视图离屏目标：最终 color target 和 raster depth target。
///
/// color target 是 per-frame 的，因为 compute graph 与 present graph 会围绕当前 frame label
/// 读写它；depth target 目前是单张窗口尺寸资源，作为 raster pass 的 depth attachment。
/// 它们属于 app 的主视图策略，不进入 engine `GpuStore`。
pub struct MainViewTargets {
    color: PerFrameImageSet,
    depth: ImageTarget,
}

impl MainViewTargets {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        bindless_manager: &mut BindlessManager,
        frame_settings: &FrameSettings,
        frame_counter: &FrameCounter,
    ) -> Self {
        let color = PerFrameImageSet::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            TargetImageDesc {
                name_prefix: "main-view-color",
                format: frame_settings.color_format,
                extent: frame_settings.frame_extent,
                usage: vk::ImageUsageFlags::STORAGE
                    | vk::ImageUsageFlags::TRANSFER_SRC
                    | vk::ImageUsageFlags::SAMPLED
                    | vk::ImageUsageFlags::COLOR_ATTACHMENT,
            },
            frame_counter,
        );
        let depth = create_depth_target(resource_ctx, device_ctx, gfx_resource_manager, frame_settings, frame_counter);

        let targets = Self { color, depth };
        targets.register_bindless(bindless_manager);
        targets
    }

    pub fn rebuild(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        bindless_manager: &mut BindlessManager,
        gfx_resource_manager: &mut GfxResourceManager,
        frame_settings: &FrameSettings,
        frame_counter: &FrameCounter,
    ) {
        self.destroy(resource_ctx, device_ctx, bindless_manager, gfx_resource_manager, DestroyReason::Resize);
        *self = Self::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            bindless_manager,
            frame_settings,
            frame_counter,
        );
    }

    pub fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        bindless_manager: &mut BindlessManager,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        self.unregister_bindless(bindless_manager);
        self.color.destroy(resource_ctx, device_ctx, gfx_resource_manager, reason);
        gfx_resource_manager.release_image_immediate(resource_ctx, device_ctx, self.depth.image, reason);
        self.depth = ImageTarget::default();
    }

    #[inline]
    pub fn color(&self, frame_label: FrameLabel) -> ImageTarget {
        self.color.target(frame_label)
    }

    #[inline]
    pub fn depth(&self) -> ImageTarget {
        self.depth
    }

    fn register_bindless(&self, bindless_manager: &mut BindlessManager) {
        // main view color 既可能被 compute/post-process 写入，也会在 present/GUI 合成阶段被读取；
        // 因此同时注册 UAV 和 SRV。depth target 只作为 attachment 使用，不进入 bindless。
        self.color.register_uav(bindless_manager);
        self.color.register_srv(bindless_manager);
    }

    fn unregister_bindless(&self, bindless_manager: &mut BindlessManager) {
        self.color.unregister_uav(bindless_manager);
        self.color.unregister_srv(bindless_manager);
    }
}

impl Drop for MainViewTargets {
    fn drop(&mut self) {
        debug_assert!(self.depth.image.is_null());
        debug_assert!(self.depth.view.is_null());
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

struct TargetImageDesc<'a> {
    /// 资源名用于 debug name、Tracy span 和 destroy 日志定位。
    name_prefix: &'a str,
    /// image 与 view 使用同一格式；调用方负责保证 pipeline attachment 格式匹配。
    format: vk::Format,
    /// 创建时的窗口尺寸快照，resize 后必须重建 owner。
    extent: vk::Extent2D,
    /// 由具体 target 语义决定的 Vulkan usage，不在 engine 中硬编码。
    usage: vk::ImageUsageFlags,
}

fn create_single_color_target(
    resource_ctx: GfxResourceCtx<'_>,
    device_ctx: GfxDeviceCtx<'_>,
    immediate_ctx: GfxImmediateCtx<'_>,
    gfx_resource_manager: &mut GfxResourceManager,
    desc: TargetImageDesc<'_>,
    frame_counter: &FrameCounter,
) -> ImageTarget {
    let image = create_image(
        resource_ctx,
        desc.extent,
        desc.format,
        desc.usage,
        format!("{}-{}", desc.name_prefix, frame_counter.frame_id()),
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
        format!("{}-{}", desc.name_prefix, frame_counter.frame_id()),
    );

    ImageTarget {
        image: image_handle,
        view: view_handle,
        format: desc.format,
        extent: desc.extent,
    }
}

fn create_depth_target(
    resource_ctx: GfxResourceCtx<'_>,
    device_ctx: GfxDeviceCtx<'_>,
    gfx_resource_manager: &mut GfxResourceManager,
    frame_settings: &FrameSettings,
    frame_counter: &FrameCounter,
) -> ImageTarget {
    let image = create_image(
        resource_ctx,
        frame_settings.frame_extent,
        frame_settings.depth_format,
        vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
        format!("main-view-depth-{}", frame_counter.frame_id()),
    );
    let image_handle = gfx_resource_manager.register_image(image);
    let view_handle = gfx_resource_manager.get_or_create_image_view(
        device_ctx,
        image_handle,
        GfxImageViewDesc::new_2d(frame_settings.depth_format, vk::ImageAspectFlags::DEPTH),
        format!("main-view-depth-{}", frame_counter.frame_id()),
    );

    ImageTarget {
        image: image_handle,
        view: view_handle,
        format: frame_settings.depth_format,
        extent: frame_settings.frame_extent,
    }
}

fn create_image(
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
    // storage/bindless target 在本项目里以 GENERAL 作为初始稳定布局，
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
