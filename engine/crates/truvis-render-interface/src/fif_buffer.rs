use ash::vk;
use itertools::Itertools;
use slotmap::Key;

use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::{
    commands::barrier::GfxImageBarrier,
    gfx::Gfx,
    resources::image::{GfxImage, GfxImageCreateInfo},
};
use crate::bindless_manager::BindlessManager;
use crate::frame_counter::FrameCounter;
use crate::gfx_resource_manager::GfxResourceManager;
use crate::handles::{GfxImageHandle, GfxImageViewHandle};
use crate::pipeline_settings::{FrameLabel, FrameSettings};

// TODO FifBuffers 放到 app 里面去，由 App 进行管理
/// 所有帧会用到的 buffers
pub struct FifBuffers {
    /// RT 单帧输出结果，每帧一个
    single_frame_rt_images: [GfxImageHandle; FrameCounter::fif_count()],
    single_frame_rt_views: [GfxImageViewHandle; FrameCounter::fif_count()],
    single_frame_format: vk::Format,
    #[allow(dead_code)]
    single_frame_extent: vk::Extent2D,

    /// RT 计算的累积结果
    pub accum_image: GfxImageHandle,
    pub accum_image_view: GfxImageViewHandle,
    accum_format: vk::Format,
    accum_extent: vk::Extent2D,

    pub depth_image: GfxImageHandle,
    pub depth_image_view: GfxImageViewHandle,
    #[allow(dead_code)]
    depth_format: vk::Format,
    #[allow(dead_code)]
    depth_extent: vk::Extent2D,

    /// 离屏渲染的结果，数量和 fif 相同
    pub off_screen_target_image_handles: [GfxImageHandle; FrameCounter::fif_count()],
    pub off_screen_target_view_handles: [GfxImageViewHandle; FrameCounter::fif_count()],
    render_target_format: vk::Format,
    #[allow(dead_code)]
    render_target_extent: vk::Extent2D,

    // ========== GBuffer ==========
    /// GBufferA: normal.xyz + roughness (R16G16B16A16_SFLOAT)
    gbuffer_a_images: [GfxImageHandle; FrameCounter::fif_count()],
    gbuffer_a_views: [GfxImageViewHandle; FrameCounter::fif_count()],
    /// GBufferB: world_position.xyz + linear_depth (R16G16B16A16_SFLOAT)
    gbuffer_b_images: [GfxImageHandle; FrameCounter::fif_count()],
    gbuffer_b_views: [GfxImageViewHandle; FrameCounter::fif_count()],
    /// GBufferC: albedo.rgb + metallic (R8G8B8A8_UNORM)
    gbuffer_c_images: [GfxImageHandle; FrameCounter::fif_count()],
    gbuffer_c_views: [GfxImageViewHandle; FrameCounter::fif_count()],
    gbuffer_extent: vk::Extent2D,
}
// new & init
impl FifBuffers {
    pub fn new(
        frame_settigns: &FrameSettings,
        bindless_manager: &mut BindlessManager,
        gfx_resource_manager: &mut GfxResourceManager,
        frame_counter: &FrameCounter,
    ) -> Self {
        // 创建 per-frame 的单帧 RT 输出图像
        let single_frame_format = frame_settigns.color_format;
        let single_frame_extent = frame_settigns.frame_extent;
        let (single_frame_rt_images, single_frame_rt_views) = Self::create_single_frame_rt_images(
            gfx_resource_manager,
            single_frame_format,
            single_frame_extent,
            frame_counter,
        );

        let accum_format = frame_settigns.color_format;
        let accum_extent = frame_settigns.frame_extent;
        let (color_image, color_image_view) =
            Self::create_color_image(gfx_resource_manager, accum_format, accum_extent, frame_counter);

        let depth_format = frame_settigns.depth_format;
        let depth_extent = frame_settigns.frame_extent;
        let (depth_image, depth_image_view) =
            Self::create_depth_image(gfx_resource_manager, depth_format, depth_extent, frame_counter);

        let render_target_format = frame_settigns.color_format;
        let render_target_extent = frame_settigns.frame_extent;
        let (render_target_image_handles, render_target_image_view_handles) = Self::create_render_targets(
            gfx_resource_manager,
            render_target_format,
            render_target_extent,
            frame_counter,
        );

        // 创建 GBuffer 图像
        let gbuffer_extent = frame_settigns.frame_extent;
        let (gbuffer_a_images, gbuffer_a_views) = Self::create_gbuffer_images(
            gfx_resource_manager,
            vk::Format::R16G16B16A16_SFLOAT,
            gbuffer_extent,
            frame_counter,
            "gbuffer-a",
        );
        let (gbuffer_b_images, gbuffer_b_views) = Self::create_gbuffer_images(
            gfx_resource_manager,
            vk::Format::R16G16B16A16_SFLOAT,
            gbuffer_extent,
            frame_counter,
            "gbuffer-b",
        );
        let (gbuffer_c_images, gbuffer_c_views) = Self::create_gbuffer_images(
            gfx_resource_manager,
            vk::Format::R8G8B8A8_UNORM,
            gbuffer_extent,
            frame_counter,
            "gbuffer-c",
        );

        let fif_buffers = Self {
            single_frame_rt_images,
            single_frame_rt_views,
            single_frame_format,
            single_frame_extent,

            accum_image: color_image,
            accum_image_view: color_image_view,
            accum_format,
            accum_extent,

            depth_image,
            depth_image_view,
            depth_extent,
            depth_format,

            off_screen_target_image_handles: render_target_image_handles,
            off_screen_target_view_handles: render_target_image_view_handles,
            render_target_format,
            render_target_extent,

            gbuffer_a_images,
            gbuffer_a_views,
            gbuffer_b_images,
            gbuffer_b_views,
            gbuffer_c_images,
            gbuffer_c_views,
            gbuffer_extent,
        };
        fif_buffers.register_bindless(bindless_manager);
        fif_buffers
    }

    /// 尺寸发生变化时，需要重新创建相关的资源
    pub fn rebuild(
        &mut self,
        bindless_manager: &mut BindlessManager,
        gfx_resource_manager: &mut GfxResourceManager,
        frame_settings: &FrameSettings,
        frame_counter: &FrameCounter,
    ) {
        self.destroy_mut(bindless_manager, gfx_resource_manager);
        *self = Self::new(frame_settings, bindless_manager, gfx_resource_manager, frame_counter);
    }

    fn register_bindless(&self, bindless_manager: &mut BindlessManager) {
        for single_frame in &self.single_frame_rt_views {
            bindless_manager.register_uav(*single_frame);
        }
        bindless_manager.register_uav(self.accum_image_view);
        for render_target in &self.off_screen_target_view_handles {
            bindless_manager.register_uav(*render_target);
            bindless_manager.register_srv(*render_target);
        }
        // 注册 GBuffer
        for gbuffer_view in &self.gbuffer_a_views {
            bindless_manager.register_uav(*gbuffer_view);
        }
        for gbuffer_view in &self.gbuffer_b_views {
            bindless_manager.register_uav(*gbuffer_view);
        }
        for gbuffer_view in &self.gbuffer_c_views {
            bindless_manager.register_uav(*gbuffer_view);
        }
    }

    fn unregister_bindless(&self, bindless_manager: &mut BindlessManager) {
        for single_frame in &self.single_frame_rt_views {
            bindless_manager.unregister_uav(*single_frame);
        }
        bindless_manager.unregister_uav(self.accum_image_view);
        for render_target in &self.off_screen_target_view_handles {
            bindless_manager.unregister_uav(*render_target);
            bindless_manager.unregister_srv(*render_target);
        }
        // 取消注册 GBuffer
        for gbuffer_view in &self.gbuffer_a_views {
            bindless_manager.unregister_uav(*gbuffer_view);
        }
        for gbuffer_view in &self.gbuffer_b_views {
            bindless_manager.unregister_uav(*gbuffer_view);
        }
        for gbuffer_view in &self.gbuffer_c_views {
            bindless_manager.unregister_uav(*gbuffer_view);
        }
    }

    /// 创建 per-frame 的单帧 RT 输出图像
    fn create_single_frame_rt_images(
        gfx_resource_manager: &mut GfxResourceManager,
        format: vk::Format,
        extent: vk::Extent2D,
        frame_counter: &FrameCounter,
    ) -> ([GfxImageHandle; FrameCounter::fif_count()], [GfxImageViewHandle; FrameCounter::fif_count()]) {
        let create_one_image = |frame_label: FrameLabel| {
            let name = format!("single-frame-rt-{}-{}", frame_label, frame_counter.frame_id());

            let image_create_info = GfxImageCreateInfo::new_image_2d_info(
                extent,
                format,
                vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::SAMPLED,
            );

            GfxImage::new(
                &image_create_info,
                &vk_mem::AllocationCreateInfo {
                    usage: vk_mem::MemoryUsage::AutoPreferDevice,
                    ..Default::default()
                },
                &name,
            )
        };
        let images = FrameCounter::frame_labes().map(create_one_image);

        // 将 layout 设置为 general
        Gfx::get().one_time_exec(
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
            "transfer-single-frame-rt-image-layout",
        );

        let image_handles = images.map(|image| gfx_resource_manager.register_image(image));
        let image_view_handles = FrameCounter::frame_labes().map(|frame_label| {
            gfx_resource_manager.get_or_create_image_view(
                image_handles[*frame_label],
                GfxImageViewDesc::new_2d(format, vk::ImageAspectFlags::COLOR),
                format!("single-frame-rt-{}-{}", frame_label, frame_counter.frame_id()),
            )
        });

        (image_handles, image_view_handles)
    }

    /// 创建 RayTracing 需要的 image
    fn create_color_image(
        gfx_resource_manager: &mut GfxResourceManager,
        format: vk::Format,
        extent: vk::Extent2D,
        frame_counter: &FrameCounter,
    ) -> (GfxImageHandle, GfxImageViewHandle) {
        let color_image_create_info = GfxImageCreateInfo::new_image_2d_info(
            extent,
            format,
            vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::SAMPLED,
        );

        let color_image = GfxImage::new(
            &color_image_create_info,
            &vk_mem::AllocationCreateInfo {
                usage: vk_mem::MemoryUsage::AutoPreferDevice,
                ..Default::default()
            },
            &format!("fif-buffer-color-{}", frame_counter.frame_id()),
        );

        // 将 layout 设置为 general
        Gfx::get().one_time_exec(
            |cmd| {
                cmd.image_memory_barrier(
                    vk::DependencyFlags::empty(),
                    &[GfxImageBarrier::new()
                        .image(color_image.handle())
                        .src_mask(vk::PipelineStageFlags2::TOP_OF_PIPE, vk::AccessFlags2::empty())
                        .dst_mask(vk::PipelineStageFlags2::BOTTOM_OF_PIPE, vk::AccessFlags2::empty())
                        .layout_transfer(vk::ImageLayout::UNDEFINED, vk::ImageLayout::GENERAL)
                        .image_aspect_flag(vk::ImageAspectFlags::COLOR)],
                );
            },
            "transfer-fif-buffer-color-image-layout",
        );

        let color_image_handle = gfx_resource_manager.register_image(color_image);
        let color_image_view_handle = gfx_resource_manager.get_or_create_image_view(
            color_image_handle,
            GfxImageViewDesc::new_2d(format, vk::ImageAspectFlags::COLOR),
            format!("fif-buffer-color-{}", frame_counter.frame_id()),
        );

        (color_image_handle, color_image_view_handle)
    }

    fn create_depth_image(
        gfx_resource_manager: &mut GfxResourceManager,
        format: vk::Format,
        extent: vk::Extent2D,
        frame_counter: &FrameCounter,
    ) -> (GfxImageHandle, GfxImageViewHandle) {
        let depth_image_create_info =
            GfxImageCreateInfo::new_image_2d_info(extent, format, vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT);
        let depth_image = GfxImage::new(
            &depth_image_create_info,
            &vk_mem::AllocationCreateInfo {
                usage: vk_mem::MemoryUsage::AutoPreferDevice,
                ..Default::default()
            },
            &format!("fif-buffer-depth-{}", frame_counter.frame_id()),
        );
        let depth_image_handle = gfx_resource_manager.register_image(depth_image);
        let depth_image_view_handle = gfx_resource_manager.get_or_create_image_view(
            depth_image_handle,
            GfxImageViewDesc::new_2d(format, vk::ImageAspectFlags::DEPTH),
            format!("fif-buffer-depth-{}", frame_counter.frame_id()),
        );

        (depth_image_handle, depth_image_view_handle)
    }

    fn create_render_targets(
        gfx_resource_manager: &mut GfxResourceManager,
        format: vk::Format,
        extent: vk::Extent2D,
        frame_counter: &FrameCounter,
    ) -> ([GfxImageHandle; FrameCounter::fif_count()], [GfxImageViewHandle; FrameCounter::fif_count()]) {
        let create_one_target = |fif_labe: FrameLabel| {
            let name = format!("render-target-{}-{}", fif_labe, frame_counter.frame_id());

            let image_create_info = GfxImageCreateInfo::new_image_2d_info(
                extent,
                format,
                vk::ImageUsageFlags::STORAGE
                    | vk::ImageUsageFlags::TRANSFER_SRC
                    | vk::ImageUsageFlags::SAMPLED
                    | vk::ImageUsageFlags::COLOR_ATTACHMENT,
            );

            GfxImage::new(
                &image_create_info,
                &vk_mem::AllocationCreateInfo {
                    usage: vk_mem::MemoryUsage::AutoPreferDevice,
                    ..Default::default()
                },
                &name,
            )
        };
        let images = FrameCounter::frame_labes().map(create_one_target);

        // 将 layout 设置为 general
        Gfx::get().one_time_exec(
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
            "transfer-fif-buffer-render-target-layout",
        );

        let image_handles = images.map(|image| gfx_resource_manager.register_image(image));
        let image_view_handles = FrameCounter::frame_labes().map(|frame_label| {
            gfx_resource_manager.get_or_create_image_view(
                image_handles[*frame_label],
                GfxImageViewDesc::new_2d(format, vk::ImageAspectFlags::COLOR),
                format!("render-target-{}-{}", frame_label, frame_counter.frame_id()),
            )
        });

        (image_handles, image_view_handles)
    }

    /// 创建 per-frame 的 GBuffer 图像
    /// - GBufferA (R16G16B16A16_SFLOAT): normal.xyz + roughness
    /// - GBufferB (R16G16B16A16_SFLOAT): world_position.xyz + linear_depth
    /// - GBufferC (R8G8B8A8_UNORM): albedo.rgb + metallic
    fn create_gbuffer_images(
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
                &image_create_info,
                &vk_mem::AllocationCreateInfo {
                    usage: vk_mem::MemoryUsage::AutoPreferDevice,
                    ..Default::default()
                },
                &name,
            )
        };
        let images = FrameCounter::frame_labes().map(create_one_image);

        // 将 layout 设置为 general（用于 storage image）
        Gfx::get().one_time_exec(
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
                image_handles[*frame_label],
                GfxImageViewDesc::new_2d(format, vk::ImageAspectFlags::COLOR),
                format!("{}-{}-{}", name_prefix, frame_label, frame_counter.frame_id()),
            )
        });

        (image_handles, image_view_handles)
    }
}
// destroy
impl FifBuffers {
    pub fn destroy_mut(
        &mut self,
        bindless_manager: &mut BindlessManager,
        gfx_resource_manager: &mut GfxResourceManager,
    ) {
        self.unregister_bindless(bindless_manager);

        // 只需销毁 image，view 会跟随销毁
        for single_frame_image in std::mem::take(&mut self.single_frame_rt_images) {
            gfx_resource_manager.destroy_image_immediate(single_frame_image);
        }
        for render_target_image in std::mem::take(&mut self.off_screen_target_image_handles) {
            gfx_resource_manager.destroy_image_immediate(render_target_image);
        }

        // 销毁 GBuffer 图像
        for gbuffer_image in std::mem::take(&mut self.gbuffer_a_images) {
            gfx_resource_manager.destroy_image_immediate(gbuffer_image);
        }
        for gbuffer_image in std::mem::take(&mut self.gbuffer_b_images) {
            gfx_resource_manager.destroy_image_immediate(gbuffer_image);
        }
        for gbuffer_image in std::mem::take(&mut self.gbuffer_c_images) {
            gfx_resource_manager.destroy_image_immediate(gbuffer_image);
        }

        // image view 无需销毁，只需要销毁 image 即可
        gfx_resource_manager.destroy_image_immediate(self.depth_image);
        gfx_resource_manager.destroy_image_immediate(self.accum_image);

        self.single_frame_rt_views = Default::default();
        self.depth_image_view = GfxImageViewHandle::default();
        self.accum_image_view = GfxImageViewHandle::default();
        self.depth_image = GfxImageHandle::default();
        self.accum_image = GfxImageHandle::default();
        self.gbuffer_a_views = Default::default();
        self.gbuffer_b_views = Default::default();
        self.gbuffer_c_views = Default::default();
    }
}
impl Drop for FifBuffers {
    fn drop(&mut self) {
        debug_assert!(self.single_frame_rt_images.iter().all(|img| img.is_null()));
        debug_assert!(self.off_screen_target_image_handles.iter().all(|target| target.is_null()));
        debug_assert!(self.gbuffer_a_images.iter().all(|img| img.is_null()));
        debug_assert!(self.gbuffer_b_images.iter().all(|img| img.is_null()));
        debug_assert!(self.gbuffer_c_images.iter().all(|img| img.is_null()));
        debug_assert!(self.depth_image.is_null());
        debug_assert!(self.depth_image_view.is_null());
        debug_assert!(self.accum_image.is_null());
        debug_assert!(self.accum_image_view.is_null());
    }
}
// getter
impl FifBuffers {
    /// 获取当前帧的单帧 RT 输出图像句柄
    #[inline]
    pub fn single_frame_rt_handle(&self, frame_label: FrameLabel) -> (GfxImageHandle, GfxImageViewHandle) {
        (self.single_frame_rt_images[*frame_label], self.single_frame_rt_views[*frame_label])
    }

    /// 获取单帧 RT 输出图像的格式
    #[inline]
    pub fn single_frame_rt_format(&self) -> vk::Format {
        self.single_frame_format
    }

    #[inline]
    pub fn depth_image_view_handle(&self) -> GfxImageViewHandle {
        self.depth_image_view
    }

    #[inline]
    pub fn render_target_handle(&self, frame_label: FrameLabel) -> (GfxImageHandle, GfxImageViewHandle) {
        (
            self.off_screen_target_image_handles[frame_label as usize],
            self.off_screen_target_view_handles[frame_label as usize],
        )
    }

    /// 获取累积图像句柄
    #[inline]
    pub fn accum_image_handle(&self) -> GfxImageHandle {
        self.accum_image
    }

    /// 获取累积图像视图句柄
    #[inline]
    pub fn accum_image_view_handle(&self) -> GfxImageViewHandle {
        self.accum_image_view
    }

    /// 获取累积图像格式
    #[inline]
    pub fn accum_image_format(&self) -> vk::Format {
        self.accum_format
    }

    /// 获取累积图像尺寸
    #[inline]
    pub fn accum_image_extent(&self) -> vk::Extent2D {
        self.accum_extent
    }

    // 保留旧的 color_* 方法作为别名以保持兼容性
    #[inline]
    #[deprecated(note = "use accum_image_handle() instead")]
    pub fn color_image_handle(&self) -> GfxImageHandle {
        self.accum_image
    }

    #[inline]
    #[deprecated(note = "use accum_image_view_handle() instead")]
    pub fn color_image_view_handle(&self) -> GfxImageViewHandle {
        self.accum_image_view
    }

    #[inline]
    #[deprecated(note = "use accum_image_format() instead")]
    pub fn color_image_format(&self) -> vk::Format {
        self.accum_format
    }

    #[inline]
    #[deprecated(note = "use accum_image_extent() instead")]
    pub fn color_image_extent(&self) -> vk::Extent2D {
        self.accum_extent
    }

    #[inline]
    pub fn render_target_format(&self) -> vk::Format {
        self.render_target_format
    }

    // ========== GBuffer Getters ==========

    /// 获取 GBufferA (normal.xyz + roughness) 的 handle
    #[inline]
    pub fn gbuffer_a_handle(&self, frame_label: FrameLabel) -> (GfxImageHandle, GfxImageViewHandle) {
        (self.gbuffer_a_images[*frame_label], self.gbuffer_a_views[*frame_label])
    }

    /// 获取 GBufferB (world_position.xyz + linear_depth) 的 handle
    #[inline]
    pub fn gbuffer_b_handle(&self, frame_label: FrameLabel) -> (GfxImageHandle, GfxImageViewHandle) {
        (self.gbuffer_b_images[*frame_label], self.gbuffer_b_views[*frame_label])
    }

    /// 获取 GBufferC (albedo.rgb + metallic) 的 handle
    #[inline]
    pub fn gbuffer_c_handle(&self, frame_label: FrameLabel) -> (GfxImageHandle, GfxImageViewHandle) {
        (self.gbuffer_c_images[*frame_label], self.gbuffer_c_views[*frame_label])
    }

    /// 获取 GBuffer 的尺寸
    #[inline]
    pub fn gbuffer_extent(&self) -> vk::Extent2D {
        self.gbuffer_extent
    }

    /// GBufferA 格式: R16G16B16A16_SFLOAT
    #[inline]
    pub const fn gbuffer_a_format() -> vk::Format {
        vk::Format::R16G16B16A16_SFLOAT
    }

    /// GBufferB 格式: R16G16B16A16_SFLOAT
    #[inline]
    pub const fn gbuffer_b_format() -> vk::Format {
        vk::Format::R16G16B16A16_SFLOAT
    }

    /// GBufferC 格式: R8G8B8A8_UNORM
    #[inline]
    pub const fn gbuffer_c_format() -> vk::Format {
        vk::Format::R8G8B8A8_UNORM
    }
}
