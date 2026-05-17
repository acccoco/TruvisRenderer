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
    /// 当前 swapchain image 在资源管理器中的 image handle。
    ///
    /// RenderGraph pass 通过该 handle 把最终结果写入或拷贝到窗口图像；handle 只包装 WSI image，
    /// 不拥有 Vulkan image 本体。
    pub render_target_image_handle: GfxImageHandle,
    /// 当前 swapchain image 对应的 view handle，随 swapchain resize 重建。
    pub render_target_view_handle: GfxImageViewHandle,

    /// 从 render graph 写入 present target 前应使用的同步范围。
    ///
    /// 具体 pass 决定 layout transition 和读写操作，这里只提供 backend 对窗口图像的同步契约。
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
    /// 创建 surface、swapchain、swapchain image/view wrapper 和 present 同步对象。
    ///
    /// 该函数只能在平台层提供 raw window/display handle 后调用。创建出的 swapchain images
    /// 会注册到 `GfxResourceManager`，但 Vulkan image 本体仍由 WSI 拥有。
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
    /// 返回当前 acquire 到的 swapchain image/view handle。
    ///
    /// 只有 `acquire_image` 成功后才有明确的 current image index；调用者通常在 render 阶段读取它。
    pub fn current_image_and_view(&self) -> (GfxImageHandle, GfxImageViewHandle) {
        let swapchain = self.swapchain.as_ref().unwrap();
        let image_idx = swapchain.current_image_index();

        (self.swapchain_images[image_idx], self.swapchain_image_views[image_idx])
    }

    /// 返回当前 swapchain 的 image 数量、格式和 extent。
    ///
    /// init/resize 阶段用它同步上层窗口尺寸相关资源。
    #[inline]
    pub fn swapchain_image_info(&self) -> GfxSwapchainImageInfo {
        self.swapchain.as_ref().unwrap().image_infos()
    }

    /// 当前 swapchain image 对应的 render-complete semaphore。
    ///
    /// render graph 提交完成后 signal 它，`present_image` 会在 present queue 上等待同一个 semaphore。
    #[inline]
    pub fn current_render_compute_semaphore(&self) -> &GfxSemaphore {
        let swapchain = self.swapchain.as_ref().unwrap();
        &self.render_complete_semaphores[swapchain.current_image_index()]
    }

    /// 当前 FIF frame label 对应的 acquire-complete semaphore。
    ///
    /// `acquire_image` signal 它，render graph 在开始写当前 present target 前应等待它。
    #[inline]
    pub fn current_present_complete_semaphore(&self, frame_label: FrameLabel) -> &GfxSemaphore {
        &self.present_complete_semaphores[*frame_label]
    }
}

// 更新
impl RenderPresent {
    /// 记录窗口的最新物理尺寸，并标记后续帧需要检查 swapchain 重建。
    ///
    /// 这里只更新 latest-size 状态，不立即触碰 Vulkan 对象；真正重建发生在 render loop
    /// 能够安全等待 device idle 的 resize 路径中。
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

    /// 判断当前 latest-size 标记是否真的需要重建 swapchain。
    ///
    /// 需要综合窗口事件记录的物理尺寸与 surface capabilities。某些平台会 clamp extent；
    /// 如果 clamp 后与当前 swapchain 相同，这次 resize 会被吞掉。
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

    /// acquire 当前 frame label 的 swapchain image。
    ///
    /// acquire semaphore 按 FIF 分配，和 command pool/reset 的复用节奏一致；返回的 current image index
    /// 决定本帧 `current_image_and_view` 与 render-complete semaphore。
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

    /// 将当前 swapchain image 提交给 present queue。
    ///
    /// 这里不提交渲染命令，只等待 render graph signal 的当前 image render-complete semaphore。
    /// 如果 WSI 返回 out-of-date/suboptimal，resize 标记会留给后续帧处理。
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
    /// 释放 swapchain image wrapper/view、同步对象、swapchain 与 surface。
    ///
    /// swapchain image 本体由 WSI 拥有，资源管理器释放的是 tracking wrapper 和 image view；
    /// surface 必须在 swapchain 之后销毁。
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
