use ash::vk;
use ash::vk::Handle;
use itertools::Itertools;

use crate::commands::command_queue::GfxCommandQueue;
use crate::commands::fence::GfxFence;
use crate::commands::semaphore::GfxSemaphore;
use crate::gfx::Gfx;
use crate::swapchain::surface::GfxSurface;

pub struct GfxSwapchain {
    swapchain_handle: vk::SwapchainKHR,

    swapchain_images: Vec<vk::Image>,
    swapchain_image_index: usize,

    format: vk::Format,
    swapchain_extent: vk::Extent2D,
}

// new & init
impl GfxSwapchain {
    pub fn new(
        surface: &GfxSurface,
        present_mode: vk::PresentModeKHR,
        surface_format: vk::SurfaceFormatKHR,
        window_physical_extent: vk::Extent2D,
        old_swapchain: Option<GfxSwapchain>,
    ) -> Self {
        let surface_capabilities = surface.get_capabilities();

        // 确定 window 的 extent 尺寸
        // 如果 surface_capabilities.current_extent 包含特殊值 0xFFFFFFFF，则表示可以自己设置交换链的 extent
        let extent = Self::calculate_swapchain_extent(&surface_capabilities, window_physical_extent);
        log::debug!(
            "create swapchain:
            surface current extent: {}x{}, min extent: {}x{}, max extent: {}x{}
            window physical extent: {}x{}
            final swapchain extent: {}x{}",
            surface_capabilities.current_extent.width,
            surface_capabilities.current_extent.height,
            surface_capabilities.min_image_extent.width,
            surface_capabilities.min_image_extent.height,
            surface_capabilities.max_image_extent.width,
            surface_capabilities.max_image_extent.height,
            window_physical_extent.width,
            window_physical_extent.height,
            extent.width,
            extent.height
        );

        let swapchain_handle = Self::create_swapchain(
            surface,
            surface_format.format,
            surface_format.color_space,
            extent,
            present_mode,
            old_swapchain.as_ref().map(|s| s.swapchain_handle),
        );
        if let Some(old_swapchain) = old_swapchain {
            old_swapchain.destroy();
        }

        let images = unsafe { Gfx::get().gfx_device().swapchain.get_swapchain_images(swapchain_handle).unwrap() };

        Self {
            swapchain_handle,
            swapchain_images: images,
            swapchain_image_index: 0,
            swapchain_extent: extent,
            format: surface_format.format,
        }
    }

    fn create_swapchain(
        surface: &GfxSurface,
        format: vk::Format,
        color_space: vk::ColorSpaceKHR,
        extent: vk::Extent2D,
        present_mode: vk::PresentModeKHR,
        old_swapchain: Option<vk::SwapchainKHR>,
    ) -> vk::SwapchainKHR {
        // 确定 image count
        // max_image_count == 0，表示不限制 image 数量
        let surface_capabilities = surface.get_capabilities();

        let image_count = if surface_capabilities.max_image_count == 0 {
            surface_capabilities.min_image_count + 1
        } else {
            u32::min(surface_capabilities.max_image_count, surface_capabilities.min_image_count + 1)
        };

        let create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface.handle)
            .min_image_count(image_count)
            .image_format(format)
            .image_color_space(color_space)
            .image_extent(extent)
            .image_array_layers(1)
            // TRANSFER_DST 用于 Nsight 分析
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST)
            .pre_transform(surface_capabilities.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present_mode)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .old_swapchain(old_swapchain.unwrap_or_default())
            .clipped(true);

        let gfx_device = Gfx::get().gfx_device();
        unsafe {
            let swapchain_handle = gfx_device.swapchain.create_swapchain(&create_info, None).unwrap();
            gfx_device.set_object_debug_name(swapchain_handle, "main");

            swapchain_handle
        }
    }
}

pub struct GfxSwapchainImageInfo {
    pub image_extent: vk::Extent2D,
    pub image_cnt: usize,
    pub image_format: vk::Format,
}

// getters
impl GfxSwapchain {
    #[inline]
    pub fn present_images(&self) -> Vec<vk::Image> {
        self.swapchain_images.clone()
    }

    #[inline]
    pub fn extent(&self) -> vk::Extent2D {
        self.swapchain_extent
    }

    #[inline]
    pub fn current_image_index(&self) -> usize {
        self.swapchain_image_index
    }

    #[inline]
    pub fn image_infos(&self) -> GfxSwapchainImageInfo {
        GfxSwapchainImageInfo {
            image_extent: self.swapchain_extent,
            image_cnt: self.swapchain_images.len(),
            image_format: self.format,
        }
    }
}

// tools
impl GfxSwapchain {
    /// 确定 window 的 extent 尺寸
    ///
    /// 如果 surface_capabilities.current_extent 包含特殊值 0xFFFFFFFF，则表示可以自己设置交换链的 extent
    pub fn calculate_swapchain_extent(
        surface_capabilities: &vk::SurfaceCapabilitiesKHR,
        window_physical_extent: vk::Extent2D,
    ) -> vk::Extent2D {
        let surface_extent = surface_capabilities.current_extent;
        if surface_extent.width == 0xFFFFFFFF || surface_extent.height == 0xFFFFFFFF {
            let width = window_physical_extent
                .width
                .clamp(surface_capabilities.min_image_extent.width, surface_capabilities.max_image_extent.width);
            let height = window_physical_extent
                .height
                .clamp(surface_capabilities.min_image_extent.height, surface_capabilities.max_image_extent.height);
            vk::Extent2D { width, height }
        } else {
            surface_extent
        }
    }
}

// update
impl GfxSwapchain {
    /// timeout: nano seconds
    /// return: need recreate
    #[inline]
    pub fn acquire_next_image(
        &mut self,
        semaphore: Option<&GfxSemaphore>,
        fence: Option<&GfxFence>,
        timeout: u64,
    ) -> bool {
        let result = unsafe {
            Gfx::get().gfx_device().swapchain.acquire_next_image(
                self.swapchain_handle,
                timeout,
                semaphore.map_or(vk::Semaphore::null(), |s| s.handle()),
                fence.map_or(vk::Fence::null(), |f| f.handle()),
            )
        };

        match result {
            Ok((image_index, is_suboptimal)) => {
                if is_suboptimal {
                    log::warn!("swapchain acquire image index {} is not optimal", image_index);
                }
                self.swapchain_image_index = image_index as usize;
                is_suboptimal
            }
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                log::warn!("swapchain is out of date when acquire next image");
                true
            }
            Err(e) => {
                panic!("failed to acquire next swapchain image: {:?}", e);
            }
        }
    }

    /// return: need recreate
    #[inline]
    pub fn present_image(&self, queue: &GfxCommandQueue, wait_semaphores: &[GfxSemaphore]) -> bool {
        let wait_semaphores = wait_semaphores.iter().map(|s| s.handle()).collect_vec();
        let image_indices = [self.swapchain_image_index as u32];
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(&wait_semaphores)
            .image_indices(&image_indices)
            .swapchains(std::slice::from_ref(&self.swapchain_handle));

        let result = unsafe { Gfx::get().gfx_device().swapchain.queue_present(queue.handle(), &present_info) };
        match result {
            Ok(is_suboptimal) => {
                if is_suboptimal {
                    log::warn!("swapchain present image index {} is not optimal", self.swapchain_image_index);
                }
                is_suboptimal
            }
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                log::warn!("swapchain is out of date when present image");
                true
            }
            Err(e) => {
                panic!("failed to present swapchain image: {:?}", e);
            }
        }
    }
}

// destroy
impl GfxSwapchain {
    pub fn destroy(mut self) {
        unsafe {
            let gfx_device = Gfx::get().gfx_device();
            gfx_device.swapchain.destroy_swapchain(self.swapchain_handle, None);
        }
        self.swapchain_handle = vk::SwapchainKHR::null();
    }
}
impl Drop for GfxSwapchain {
    fn drop(&mut self) {
        assert!(self.swapchain_handle.is_null());
    }
}
