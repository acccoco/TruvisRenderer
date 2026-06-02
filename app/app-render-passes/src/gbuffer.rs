//! RT 管线的 GBuffer 资源管理。
//!
//! GBuffer 由三张 per-FIF storage 纹理组成，记录 RT 首次命中的几何与材质信息，
//! 供降噪等后处理 pass 读取。通道布局与 shader 侧 `GBufferData`（`gbuffer.slangi`）对应：
//!
//! | 通道 | 格式 | 内容 |
//! |------|------|------|
//! | A | R16G16B16A16_SFLOAT | normal.xyz + roughness |
//! | B | R16G16B16A16_SFLOAT | world_position.xyz + linear_depth |
//! | C | R8G8B8A8_UNORM | albedo.rgb + metallic |
//!
//! 生命周期由 `RtPipeline`（app-kit）管理：init 创建、on_resize 重建、shutdown 销毁。
//! engine 层不再持有 GBuffer 资源。

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
use truvis_render_foundation::pipeline_settings::FrameLabel;

/// RT 管线使用的 GBuffer 资源集合。
///
/// 持有三个通道（A/B/C）的 per-FIF storage image 和对应 image view，
/// 以及它们在 `BindlessManager` 中的 UAV 注册。RT raygen pass 写入，
/// denoise/accum compute pass 通过 bindless UAV 读取。
///
/// 格式和通道语义是管线策略决策，由 app 层决定，不属于 engine 基础设施。
pub struct GBuffer {
    /// GBufferA：法线 normal.xyz + 粗糙度 roughness (R16G16B16A16_SFLOAT)
    a_images: [GfxImageHandle; FrameCounter::fif_count()],
    a_views: [GfxImageViewHandle; FrameCounter::fif_count()],
    /// GBufferB：世界位置 world_position.xyz + 线性深度 linear_depth (R16G16B16A16_SFLOAT)
    b_images: [GfxImageHandle; FrameCounter::fif_count()],
    b_views: [GfxImageViewHandle; FrameCounter::fif_count()],
    /// GBufferC：反照率 albedo.rgb + 金属度 metallic (R8G8B8A8_UNORM)
    c_images: [GfxImageHandle; FrameCounter::fif_count()],
    c_views: [GfxImageViewHandle; FrameCounter::fif_count()],
    extent: vk::Extent2D,
}

impl GBuffer {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        bindless_manager: &mut BindlessManager,
        extent: vk::Extent2D,
        frame_counter: &FrameCounter,
    ) -> Self {
        let (a_images, a_views) = Self::create_channel_images(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            Self::A_FORMAT,
            extent,
            frame_counter,
            "gbuffer-a",
        );
        let (b_images, b_views) = Self::create_channel_images(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            Self::B_FORMAT,
            extent,
            frame_counter,
            "gbuffer-b",
        );
        let (c_images, c_views) = Self::create_channel_images(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            Self::C_FORMAT,
            extent,
            frame_counter,
            "gbuffer-c",
        );

        let gbuffer = Self {
            a_images,
            a_views,
            b_images,
            b_views,
            c_images,
            c_views,
            extent,
        };
        gbuffer.register_bindless(bindless_manager);
        gbuffer
    }

    /// 窗口尺寸变化时重建所有 GBuffer 资源。
    pub fn rebuild(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        bindless_manager: &mut BindlessManager,
        gfx_resource_manager: &mut GfxResourceManager,
        extent: vk::Extent2D,
        frame_counter: &FrameCounter,
    ) {
        self.destroy(resource_ctx, device_ctx, bindless_manager, gfx_resource_manager, DestroyReason::Resize);
        *self = Self::new(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            gfx_resource_manager,
            bindless_manager,
            extent,
            frame_counter,
        );
    }

    /// 释放所有 GBuffer GPU 资源并取消 bindless 注册。
    pub fn destroy(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        bindless_manager: &mut BindlessManager,
        gfx_resource_manager: &mut GfxResourceManager,
        reason: DestroyReason,
    ) {
        self.unregister_bindless(bindless_manager);

        for image in std::mem::take(&mut self.a_images) {
            gfx_resource_manager.release_image_immediate(resource_ctx, device_ctx, image, reason);
        }
        for image in std::mem::take(&mut self.b_images) {
            gfx_resource_manager.release_image_immediate(resource_ctx, device_ctx, image, reason);
        }
        for image in std::mem::take(&mut self.c_images) {
            gfx_resource_manager.release_image_immediate(resource_ctx, device_ctx, image, reason);
        }

        self.a_views = Default::default();
        self.b_views = Default::default();
        self.c_views = Default::default();
    }
}

// Bindless 注册
impl GBuffer {
    fn register_bindless(&self, bindless_manager: &mut BindlessManager) {
        for view in &self.a_views {
            bindless_manager.register_uav(*view);
        }
        for view in &self.b_views {
            bindless_manager.register_uav(*view);
        }
        for view in &self.c_views {
            bindless_manager.register_uav(*view);
        }
    }

    fn unregister_bindless(&self, bindless_manager: &mut BindlessManager) {
        for view in &self.a_views {
            bindless_manager.unregister_uav(*view);
        }
        for view in &self.b_views {
            bindless_manager.unregister_uav(*view);
        }
        for view in &self.c_views {
            bindless_manager.unregister_uav(*view);
        }
    }
}

// 访问器与格式常量
impl GBuffer {
    pub const A_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
    pub const B_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
    pub const C_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

    /// GBufferA（法线 + 粗糙度）的当前帧 image 和 view handle。
    #[inline]
    pub fn a_handle(&self, frame_label: FrameLabel) -> (GfxImageHandle, GfxImageViewHandle) {
        (self.a_images[*frame_label], self.a_views[*frame_label])
    }

    /// GBufferB（世界位置 + 线性深度）的当前帧 image 和 view handle。
    #[inline]
    pub fn b_handle(&self, frame_label: FrameLabel) -> (GfxImageHandle, GfxImageViewHandle) {
        (self.b_images[*frame_label], self.b_views[*frame_label])
    }

    /// GBufferC（反照率 + 金属度）的当前帧 image 和 view handle。
    #[inline]
    pub fn c_handle(&self, frame_label: FrameLabel) -> (GfxImageHandle, GfxImageViewHandle) {
        (self.c_images[*frame_label], self.c_views[*frame_label])
    }

    #[inline]
    pub fn extent(&self) -> vk::Extent2D {
        self.extent
    }
}

// 纹理创建
impl GBuffer {
    /// 创建单个 GBuffer 通道的 per-FIF storage image 并将 layout 转为 GENERAL。
    fn create_channel_images(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        format: vk::Format,
        extent: vk::Extent2D,
        frame_counter: &FrameCounter,
        name_prefix: &str,
    ) -> ([GfxImageHandle; FrameCounter::fif_count()], [GfxImageViewHandle; FrameCounter::fif_count()]) {
        let create_one_image = |frame_label: FrameLabel| {
            let name = format!("{}-{}-{}", name_prefix, frame_label, frame_counter.frame_id());
            let image_create_info = GfxImageCreateInfo::new_image_2d_info(
                extent,
                format,
                vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED,
            );
            GfxImage::new(
                resource_ctx,
                &image_create_info,
                &vk_mem::AllocationCreateInfo {
                    usage: vk_mem::MemoryUsage::AutoPreferDevice,
                    ..Default::default()
                },
                &name,
            )
        };
        let images = FrameCounter::frame_labes().map(create_one_image);

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
            &format!("transfer-{}-layout", name_prefix),
        );

        let image_handles = images.map(|image| gfx_resource_manager.register_image(image));
        let image_view_handles = FrameCounter::frame_labes().map(|frame_label| {
            gfx_resource_manager.get_or_create_image_view(
                device_ctx,
                image_handles[*frame_label],
                GfxImageViewDesc::new_2d(format, vk::ImageAspectFlags::COLOR),
                format!("{}-{}-{}", name_prefix, frame_label, frame_counter.frame_id()),
            )
        });

        (image_handles, image_view_handles)
    }
}

impl Drop for GBuffer {
    fn drop(&mut self) {
        debug_assert!(self.a_images.iter().all(|img| img.is_null()));
        debug_assert!(self.b_images.iter().all(|img| img.is_null()));
        debug_assert!(self.c_images.iter().all(|img| img.is_null()));
    }
}
