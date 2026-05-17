use ash::vk;
use itertools::Itertools;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use truvis_gfx::commands::barrier::GfxBarrierMask;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxQueueCtx, GfxResourceCtx, GfxSurfaceCtx};
use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_gfx::swapchain::surface::GfxSurface;
use truvis_gfx::swapchain::swapchain::{GfxSwapchain, GfxSwapchainImageInfo};
use truvis_render_interface::frame_counter::FrameCounter;
use truvis_render_interface::gfx_resource_manager::GfxResourceManager;
use truvis_render_interface::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_render_interface::pipeline_settings::{DefaultRenderBackendSettings, FrameLabel};

/// 当前 swapchain image 对应的 present target 信息。
///
/// RenderGraph 通过这里拿到本帧最终要写入/拷贝的窗口图像及其同步需求；
/// 它不拥有 swapchain，只引用 `RenderPresent` 已注册到资源管理器中的 image/view handle。
#[derive(Copy, Clone)]
pub struct PresentData {
    /// 当前帧的渲染目标纹理
    ///
    /// 包含了最终的渲染结果，将被复制或演示到屏幕上
    pub render_target_image_handle: GfxImageHandle,
    pub render_target_view_handle: GfxImageViewHandle,

    /// 渲染目标的内存屏障配置
    ///
    /// 定义了渲染目标纹理的同步需求，确保在读取前所有写入操作已完成
    pub render_target_barrier: GfxBarrierMask,
}

/// 窗口 surface、swapchain image/view 和 present 同步对象的 owner。
///
/// `RenderBackend` 只通过它 acquire/present 当前窗口图像；render pass 看到的是
/// `PresentData`/image handle，而不是直接操作 `GfxSwapchain`。
pub struct RenderPresent {
    surface: GfxSurface,
    /// swapchain 在 resize 时会被取出作为 old_swapchain 传给 Vulkan，字段使用 Option 表达重建过程中的临时空状态。
    pub swapchain: Option<GfxSwapchain>,
    /// swapchain images 是外部 WSI 对象，这里只注册 handle，销毁时从资源管理器释放 wrapper，不销毁 Vulkan image 本体。
    pub swapchain_images: Vec<GfxImageHandle>,
    pub swapchain_image_views: Vec<GfxImageViewHandle>,

    /// 数量和 FIF 数相同；acquire 当前 frame label 的 image 时 signal。
    pub present_complete_semaphores: [GfxSemaphore; FrameCounter::fif_count()],

    /// 数量和 swapchain image 数相同；render graph 提交完成后 signal，present 当前 image 时 wait。
    pub render_complete_semaphores: Vec<GfxSemaphore>,

    window_physical_extent: vk::Extent2D,
    /// latest-size 模式的 resize 标记。窗口事件只写入最新尺寸，真正重建延迟到 render loop 检查。
    need_resize: bool,
}

// 创建与初始化
impl RenderPresent {
    pub fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        surface_ctx: GfxSurfaceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
        raw_display_handle: RawDisplayHandle,
        raw_window_handle: RawWindowHandle,
        window_physical_extent: vk::Extent2D,
    ) -> Self {
        // surface/swapchain 必须在平台层提供 raw window/display handle 后创建，
        // 因此 RenderBackend::new 阶段不会碰窗口系统资源。
        let surface = GfxSurface::new(surface_ctx, raw_display_handle, raw_window_handle);
        let swapchain = GfxSwapchain::new(
            surface_ctx,
            &surface,
            DefaultRenderBackendSettings::DEFAULT_PRESENT_MODE,
            DefaultRenderBackendSettings::DEFAULT_SURFACE_FORMAT,
            window_physical_extent,
            None,
        );
        let (swapchain_image_handles, swapchain_image_view_handles) =
            Self::create_swapchain_images_and_views(resource_ctx, device_ctx, &swapchain, gfx_resource_manager);

        let swapchain_image_infos = swapchain.image_infos();

        let present_complete_semaphores = FrameCounter::frame_labes()
            .map(|frame_label| GfxSemaphore::new(device_ctx, &format!("window-present-complete-{}", frame_label)));
        let render_complete_semaphores = (0..swapchain_image_infos.image_cnt)
            .map(|i| GfxSemaphore::new(device_ctx, &format!("window-render-complete-{}", i)))
            .collect_vec();

        Self {
            surface,
            swapchain: Some(swapchain),
            swapchain_images: swapchain_image_handles,
            swapchain_image_views: swapchain_image_view_handles,

            present_complete_semaphores,
            render_complete_semaphores,

            window_physical_extent,
            need_resize: false,
        }
    }

    fn create_swapchain_images_and_views(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        swapchain: &GfxSwapchain,
        gfx_resource_manager: &mut GfxResourceManager,
    ) -> (Vec<GfxImageHandle>, Vec<GfxImageViewHandle>) {
        let mut image_handles = Vec::new();
        let mut image_view_handles = Vec::new();

        let swapchain_image_info = swapchain.image_infos();

        for (image_idx, vk_image) in swapchain.present_images().iter().enumerate() {
            // swapchain image 由 WSI 拥有，GfxImage::from_external 只创建资源系统可追踪的 wrapper。
            let image = GfxImage::from_external(
                resource_ctx,
                *vk_image,
                swapchain_image_info.image_extent.into(),
                swapchain_image_info.image_format,
                format!("swapchain-image-{}", image_idx),
            );
            let image_handle = gfx_resource_manager.register_image(image);

            let image_view_handle = gfx_resource_manager.get_or_create_image_view(
                device_ctx,
                image_handle,
                GfxImageViewDesc::new_2d(swapchain_image_info.image_format, vk::ImageAspectFlags::COLOR),
                format!("swapchain-{}", image_idx),
            );

            image_handles.push(image_handle);
            image_view_handles.push(image_view_handle);
        }

        (image_handles, image_view_handles)
    }
}

// 访问器
impl RenderPresent {
    pub fn current_image_and_view(&self) -> (GfxImageHandle, GfxImageViewHandle) {
        let swapchain = self.swapchain.as_ref().unwrap();
        let image_idx = swapchain.current_image_index();

        (self.swapchain_images[image_idx], self.swapchain_image_views[image_idx])
    }

    #[inline]
    pub fn swapchain_image_info(&self) -> GfxSwapchainImageInfo {
        self.swapchain.as_ref().unwrap().image_infos()
    }

    #[inline]
    pub fn current_render_compute_semaphore(&self) -> &GfxSemaphore {
        let swapchain = self.swapchain.as_ref().unwrap();
        &self.render_complete_semaphores[swapchain.current_image_index()]
    }

    #[inline]
    pub fn current_present_complete_semaphore(&self, frame_label: FrameLabel) -> &GfxSemaphore {
        &self.present_complete_semaphores[*frame_label]
    }
}

// 更新
impl RenderPresent {
    /// 记录窗口的最新尺寸
    #[inline]
    pub fn update_window_size(&mut self, window_physical_extent: [u32; 2]) {
        log::debug!(
            "window size change to: {}x{}, need rebuild swapchain",
            window_physical_extent[0],
            window_physical_extent[1]
        );

        self.window_physical_extent.width = window_physical_extent[0];
        self.window_physical_extent.height = window_physical_extent[1];
        self.need_resize = true;
    }

    /// 判断是否需要重建 swapchain
    ///
    /// 需要综合判断窗口尺寸是否发生变化，以及当前 surface 的实时状态
    pub fn need_resize(&mut self, surface_ctx: GfxSurfaceCtx<'_>) -> bool {
        if !self.need_resize {
            return false;
        }

        let surface_capibilities = self.surface.get_capabilities(surface_ctx);
        let expect_swapchain_extent =
            GfxSwapchain::calculate_swapchain_extent(&surface_capibilities, self.window_physical_extent);

        // 某些平台会把窗口逻辑尺寸限制到 surface capabilities；如果计算出的实际 extent
        // 与当前 swapchain 相同，就吞掉这次 resize 标记，避免无意义重建。
        if expect_swapchain_extent == self.swapchain.as_ref().unwrap().extent() {
            self.need_resize = false;
        }

        self.need_resize
    }

    pub fn rebuild_after_resized(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        surface_ctx: GfxSurfaceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
    ) {
        device_ctx.device().wait_idle();

        // 重建前先释放旧 image wrapper/view，再把旧 swapchain 交给 Vulkan 创建新 swapchain。
        // 这里 wait idle 是 resize 路径的保守同步点，防止窗口尺寸资源仍被在飞命令使用。
        for image_handle in std::mem::take(&mut self.swapchain_images) {
            gfx_resource_manager.release_image_immediate(resource_ctx, device_ctx, image_handle, DestroyReason::Resize);
        }
        let old_swapchain = self.swapchain.take();
        self.swapchain = Some(GfxSwapchain::new(
            surface_ctx,
            &self.surface,
            DefaultRenderBackendSettings::DEFAULT_PRESENT_MODE,
            DefaultRenderBackendSettings::DEFAULT_SURFACE_FORMAT,
            self.window_physical_extent,
            old_swapchain,
        ));
        (self.swapchain_images, self.swapchain_image_views) = Self::create_swapchain_images_and_views(
            resource_ctx,
            device_ctx,
            self.swapchain.as_ref().unwrap(),
            gfx_resource_manager,
        );

        self.need_resize = false;
    }

    pub fn acquire_image(&mut self, surface_ctx: GfxSurfaceCtx<'_>, frame_label: FrameLabel) {
        // acquire 使用按 FIF 分配的 semaphore，因为同一个 frame label 在 GPU 完成前不会复用。
        let swapchain = self.swapchain.as_mut().unwrap();
        let timeout_ns = 10 * 1000 * 1000 * 1000;

        self.need_resize = swapchain.acquire_next_image(
            surface_ctx,
            Some(&self.present_complete_semaphores[*frame_label]),
            None,
            timeout_ns,
        );
    }

    pub fn present_image(&mut self, surface_ctx: GfxSurfaceCtx<'_>, queue_ctx: GfxQueueCtx<'_>) {
        let swapchain = self.swapchain.as_ref().unwrap();
        // present 等待当前 swapchain image 对应的 render-complete semaphore；
        // semaphore 数量跟 image 数一致，避免同一帧中不同 image 的完成信号互相覆盖。
        self.need_resize = swapchain.present_image(
            surface_ctx,
            queue_ctx.gfx_queue(),
            std::slice::from_ref(&self.render_complete_semaphores[swapchain.current_image_index()]),
        );
    }
}

// 销毁
impl RenderPresent {
    pub fn destroy(
        self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        surface_ctx: GfxSurfaceCtx<'_>,
        gfx_resource_manager: &mut GfxResourceManager,
    ) {
        // swapchain image wrapper 必须在 swapchain 销毁前释放；surface 最后销毁。
        for semaphore in self.present_complete_semaphores {
            semaphore.destroy(device_ctx);
        }
        for semaphore in self.render_complete_semaphores {
            semaphore.destroy(device_ctx);
        }
        for image_handle in self.swapchain_images {
            gfx_resource_manager.release_image_immediate(
                resource_ctx,
                device_ctx,
                image_handle,
                DestroyReason::Shutdown,
            )
        }
        if let Some(swapchain) = self.swapchain {
            swapchain.destroy(surface_ctx);
        }

        self.surface.destroy(surface_ctx);
    }
}
