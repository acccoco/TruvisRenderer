use ash::vk;
use itertools::Itertools;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use truvis_gfx::commands::barrier::GfxBarrierMask;
use truvis_gfx::commands::semaphore::GfxSemaphore;
use truvis_gfx::gfx::Gfx;
use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageViewDesc;
use truvis_gfx::swapchain::surface::GfxSurface;
use truvis_gfx::swapchain::swapchain::{GfxSwapchain, GfxSwapchainImageInfo};
use truvis_gui_backend::gui_backend::GuiBackend;
use truvis_render_interface::frame_counter::FrameCounter;
use truvis_render_interface::gfx_resource_manager::GfxResourceManager;
use truvis_render_interface::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_render_interface::pipeline_settings::{DefaultRendererSettings, FrameLabel};

/// 渲染演示数据结构
///
/// 包含了向演示窗口提交渲染结果所需的所有数据和资源。
/// 这个结构体作为渲染器内部状态与外部演示系统之间的桥梁。
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

pub struct RenderPresent {
    surface: GfxSurface,
    pub swapchain: Option<GfxSwapchain>,
    pub swapchain_images: Vec<GfxImageHandle>,
    pub swapchain_image_views: Vec<GfxImageViewHandle>,

    pub gui_backend: GuiBackend,

    /// 数量和 fif num 相同
    pub present_complete_semaphores: [GfxSemaphore; FrameCounter::fif_count()],

    /// 数量和 swapchain image num 相同
    pub render_complete_semaphores: Vec<GfxSemaphore>,

    window_physical_extent: vk::Extent2D,
    need_resize: bool,
}

// new & init
impl RenderPresent {
    pub fn new(
        gfx_resource_manager: &mut GfxResourceManager,
        raw_display_handle: RawDisplayHandle,
        raw_window_handle: RawWindowHandle,
        window_physical_extent: vk::Extent2D,
    ) -> Self {
        let surface = GfxSurface::new(raw_display_handle, raw_window_handle);
        let swapchain = GfxSwapchain::new(
            &surface,
            DefaultRendererSettings::DEFAULT_PRESENT_MODE,
            DefaultRendererSettings::DEFAULT_SURFACE_FORMAT,
            window_physical_extent,
            None,
        );
        let (swapchain_image_handles, swapchain_image_view_handles) =
            Self::create_swapchain_images_and_views(&swapchain, gfx_resource_manager);

        let swapchain_image_infos = swapchain.image_infos();

        let gui_backend = GuiBackend::new();

        let present_complete_semaphores = FrameCounter::frame_labes()
            .map(|frame_label| GfxSemaphore::new(&format!("window-present-complete-{}", frame_label)));
        let render_complete_semaphores = (0..swapchain_image_infos.image_cnt)
            .map(|i| GfxSemaphore::new(&format!("window-render-complete-{}", i)))
            .collect_vec();

        Self {
            surface,
            swapchain: Some(swapchain),
            swapchain_images: swapchain_image_handles,
            swapchain_image_views: swapchain_image_view_handles,

            gui_backend,
            present_complete_semaphores,
            render_complete_semaphores,

            window_physical_extent,
            need_resize: false,
        }
    }

    fn create_swapchain_images_and_views(
        swapchain: &GfxSwapchain,
        gfx_resource_manager: &mut GfxResourceManager,
    ) -> (Vec<GfxImageHandle>, Vec<GfxImageViewHandle>) {
        let mut image_handles = Vec::new();
        let mut image_view_handles = Vec::new();

        let swapchain_image_info = swapchain.image_infos();

        for (image_idx, vk_image) in swapchain.present_images().iter().enumerate() {
            let image = GfxImage::from_external(
                *vk_image,
                swapchain_image_info.image_extent.into(),
                swapchain_image_info.image_format,
                format!("swapchain-image-{}", image_idx),
            );
            let image_handle = gfx_resource_manager.register_image(image);

            let image_view_handle = gfx_resource_manager.get_or_create_image_view(
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

// getter
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

// update
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
    pub fn need_resize(&mut self) -> bool {
        if !self.need_resize {
            return false;
        }

        let surface_capibilities = self.surface.get_capabilities();
        let expect_swapchain_extent =
            GfxSwapchain::calculate_swapchain_extent(&surface_capibilities, self.window_physical_extent);

        if expect_swapchain_extent == self.swapchain.as_ref().unwrap().extent() {
            self.need_resize = false;
        }

        self.need_resize
    }

    pub fn rebuild_after_resized(&mut self, gfx_resource_manager: &mut GfxResourceManager) {
        unsafe {
            Gfx::get().gfx_device().device_wait_idle().unwrap();
        }

        for image_handle in std::mem::take(&mut self.swapchain_images) {
            gfx_resource_manager.destroy_image_immediate(image_handle);
        }
        let old_swapchain = self.swapchain.take();
        self.swapchain = Some(GfxSwapchain::new(
            &self.surface,
            DefaultRendererSettings::DEFAULT_PRESENT_MODE,
            DefaultRendererSettings::DEFAULT_SURFACE_FORMAT,
            self.window_physical_extent,
            old_swapchain,
        ));
        (self.swapchain_images, self.swapchain_image_views) =
            Self::create_swapchain_images_and_views(self.swapchain.as_ref().unwrap(), gfx_resource_manager);

        self.need_resize = false;
    }

    pub fn acquire_image(&mut self, frame_label: FrameLabel) {
        // 从 swapchain 获取图像
        let swapchain = self.swapchain.as_mut().unwrap();
        let timeout_ns = 10 * 1000 * 1000 * 1000;

        self.need_resize =
            swapchain.acquire_next_image(Some(&self.present_complete_semaphores[*frame_label]), None, timeout_ns);
    }

    pub fn present_image(&mut self) {
        let swapchain = self.swapchain.as_ref().unwrap();
        self.need_resize = swapchain.present_image(
            Gfx::get().gfx_queue(),
            std::slice::from_ref(&self.render_complete_semaphores[swapchain.current_image_index()]),
        );
    }
}

// destroy
impl RenderPresent {
    pub fn destroy(self, gfx_resource_manager: &mut GfxResourceManager) {
        for semaphore in self.present_complete_semaphores {
            semaphore.destroy();
        }
        for semaphore in self.render_complete_semaphores {
            semaphore.destroy();
        }
        for image_handle in self.swapchain_images {
            gfx_resource_manager.destroy_image_immediate(image_handle)
        }
        if let Some(swapchain) = self.swapchain {
            swapchain.destroy();
        }

        // surface 可以在最后销毁
    }
}
