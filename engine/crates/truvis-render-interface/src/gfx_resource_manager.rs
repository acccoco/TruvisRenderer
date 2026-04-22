use std::collections::HashMap;

use ash::vk;
use slotmap::{SecondaryMap, SlotMap};

use truvis_gfx::resources::buffer::GfxBuffer;
use truvis_gfx::resources::image::{GfxImage, GfxImageCreateInfo};
use truvis_gfx::resources::image_view::GfxImageView;
use truvis_gfx::resources::image_view::GfxImageViewDesc;

use crate::frame_counter::FrameCounter;
use crate::handles::{GfxBufferHandle, GfxImageHandle, GfxImageViewHandle};

/// 资源管理器
///
/// 负责管理所有的 GPU 资源，包括 Buffer、Image 和 ImageView。
/// 使用 SlotMap 存储资源，对外提供轻量级的 Handle。
/// 支持资源的延迟销毁（Frames in Flight）。
pub struct GfxResourceManager {
    /// 存储所有的 Buffer 资源
    buffer_pool: SlotMap<GfxBufferHandle, GfxBuffer>,
    /// 存储所有的 Image 资源
    image_pool: SlotMap<GfxImageHandle, GfxImage>,
    /// 存储所有的 ImageView 资源
    image_view_pool: SlotMap<GfxImageViewHandle, GfxImageView>,

    /// 用于快速查询：(ImageHandle, ViewDesc) -> ImageViewHandle
    image_view_lookup: HashMap<(GfxImageHandle, GfxImageViewDesc), GfxImageViewHandle>,
    /// 用于缓存：ImageHandle -> 所有关联的 ImageViewHandle
    image_to_views: SecondaryMap<GfxImageHandle, Vec<GfxImageViewHandle>>,

    // 待销毁队列 (用于延迟销毁，例如在帧结束时)
    // (handle, frame_index)
    pending_destroy_buffers: Vec<(GfxBufferHandle, u64)>,
    pending_destroy_images: Vec<(GfxImageHandle, u64)>,

    destroyed: bool,
}
impl Default for GfxResourceManager {
    fn default() -> Self {
        Self::new()
    }
}
// new & init
impl GfxResourceManager {
    /// 创建一个新的资源管理器
    pub fn new() -> Self {
        Self {
            buffer_pool: SlotMap::with_key(),
            image_pool: SlotMap::with_key(),
            image_view_pool: SlotMap::with_key(),
            image_view_lookup: HashMap::new(),
            image_to_views: SecondaryMap::new(),

            pending_destroy_buffers: Vec::new(),
            pending_destroy_images: Vec::new(),

            destroyed: false,
        }
    }
}
// destroy
impl GfxResourceManager {
    pub fn destroy(mut self) {
        self.destroy_mut();
    }
    pub fn destroy_mut(&mut self) {
        let _span = tracy_client::span!("ResourceManager::destroy_all");

        // destroy 所有的 image views
        for (_, image_view) in self.image_view_pool.drain() {
            image_view.destroy()
        }
        self.image_view_lookup.clear();
        self.image_to_views.clear();

        // Destroy 所有的 images
        for (_, image) in self.image_pool.drain() {
            image.destroy()
        }

        // Destroy 所有的 buffers
        for (_, buffer) in self.buffer_pool.drain() {
            buffer.destroy()
        }

        // Clear pending queues
        self.pending_destroy_buffers.clear();
        self.pending_destroy_images.clear();

        #[cfg(debug_assertions)]
        {
            self.destroyed = true;
        }
    }
}
impl Drop for GfxResourceManager {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        {
            assert!(self.destroyed);
        }
    }
}
// Subsystem API
impl GfxResourceManager {
    /// 清理已过期的资源
    ///
    /// 检查待销毁队列，销毁那些已经不再被 GPU 使用的资源（即提交销毁时的帧索引 <= completed_frame_index）。
    pub fn cleanup(&mut self, current_frame_id: u64) {
        let _span = tracy_client::span!("ResourceManager::cleanup");

        const FIF: u64 = FrameCounter::fif_count() as u64;

        // 清理 buffers
        let mut buffers_to_destroy = Vec::new();
        self.pending_destroy_buffers.retain(|(buffer_handle, frame_index)| {
            if *frame_index + FIF <= current_frame_id {
                buffers_to_destroy.push(*buffer_handle);
                false
            } else {
                true
            }
        });
        for buffer_handle in buffers_to_destroy {
            if let Some(buffer) = self.buffer_pool.remove(buffer_handle) {
                buffer.destroy()
            }
        }

        // 清理 images
        let mut images_to_destroy = Vec::new();
        self.pending_destroy_images.retain(|(image_handle, frame_index)| {
            if *frame_index + FIF <= current_frame_id {
                images_to_destroy.push(*image_handle);
                false
            } else {
                true
            }
        });
        for image_handle in &images_to_destroy {
            // 先清理基于 image 创建的 image views
            if let Some(view_handles) = self.image_to_views.remove(*image_handle) {
                for image_view_handle in view_handles {
                    // 从 lookup 中移除对应条目
                    if let Some(image_view) = self.image_view_pool.get(image_view_handle) {
                        let view_desc = *image_view.desc();
                        self.image_view_lookup.remove(&(*image_handle, view_desc));
                    }
                    // 销毁 image view
                    if let Some(image_view) = self.image_view_pool.remove(image_view_handle) {
                        image_view.destroy()
                    }
                }
            }
            // 再销毁 image 本身
            if let Some(image) = self.image_pool.remove(*image_handle) {
                image.destroy()
            }
        }
    }
}
// Buffer API
impl GfxResourceManager {
    pub fn register_buffer(&mut self, buffer: GfxBuffer) -> GfxBufferHandle {
        self.buffer_pool.insert(buffer)
    }

    pub fn create_buffer(
        &mut self,
        buffer_size: vk::DeviceSize,
        buffer_usage: vk::BufferUsageFlags,
        align: Option<vk::DeviceSize>,
        mem_map: bool,
        name: impl AsRef<str>,
    ) -> GfxBufferHandle {
        let buffer = GfxBuffer::new(buffer_size, buffer_usage, align, mem_map, name.as_ref());
        self.register_buffer(buffer)
    }

    /// 获取 Buffer 资源引用
    pub fn get_buffer(&self, handle: GfxBufferHandle) -> Option<&GfxBuffer> {
        self.buffer_pool.get(handle)
    }

    /// 获取 Buffer 资源可变引用
    pub fn get_buffer_mut(&mut self, handle: GfxBufferHandle) -> Option<&mut GfxBuffer> {
        self.buffer_pool.get_mut(handle)
    }

    /// 销毁 Buffer（指定帧索引）
    ///
    /// 将 Buffer 加入待销毁队列，在 `current_frame_index` 对应的帧完成后销毁。
    pub fn destroy_buffer(&mut self, handle: GfxBufferHandle, current_frame_index: u64) {
        self.pending_destroy_buffers.push((handle, current_frame_index));
    }
}
// Image API
impl GfxResourceManager {
    pub fn register_image(&mut self, image: GfxImage) -> GfxImageHandle {
        self.image_pool.insert(image)
    }

    pub fn create_image(
        &mut self,
        image_info: &GfxImageCreateInfo,
        alloc_info: &vk_mem::AllocationCreateInfo,
        debug_name: &str,
    ) -> GfxImageHandle {
        let image = GfxImage::new(image_info, alloc_info, debug_name);
        self.register_image(image)
    }

    /// 获取 Image 资源引用
    pub fn get_image(&self, handle: GfxImageHandle) -> Option<&GfxImage> {
        self.image_pool.get(handle)
    }

    /// 销毁 Image（指定帧索引）
    ///
    /// 同时会销毁默认的 ImageView。
    pub fn destroy_image(&mut self, handle: GfxImageHandle, current_frame_index: u64) {
        self.pending_destroy_images.push((handle, current_frame_index));
    }

    /// 立即销毁 Image 及其关联的所有 ImageView
    ///
    /// 注意：调用者需要确保该资源不再被 GPU 使用，否则可能导致未定义行为。
    pub fn destroy_image_immediate(&mut self, handle: GfxImageHandle) {
        // 先清理基于 image 创建的 image views
        if let Some(view_handles) = self.image_to_views.remove(handle) {
            for image_view_handle in view_handles {
                // 从 lookup 中移除对应条目
                if let Some(image_view) = self.image_view_pool.get(image_view_handle) {
                    let view_desc = *image_view.desc();
                    self.image_view_lookup.remove(&(handle, view_desc));
                }
                // 销毁 image view
                if let Some(image_view) = self.image_view_pool.remove(image_view_handle) {
                    image_view.destroy()
                }
            }
        }

        // 从待销毁队列中移除（如果存在）
        self.pending_destroy_images.retain(|(h, _)| *h != handle);

        // 销毁 image 本身
        if let Some(image) = self.image_pool.remove(handle) {
            image.destroy()
        }
    }
}
// ImageView API
impl GfxResourceManager {
    /// 创建一个 ImageView
    pub fn get_or_create_image_view(
        &mut self,
        image_handle: GfxImageHandle,
        view_desc: GfxImageViewDesc,
        name: impl AsRef<str>,
    ) -> GfxImageViewHandle {
        let _span = tracy_client::span!("ResourceManager::create_image_view");

        // 如果已经存在相同描述的 ImageView，则直接返回
        let lookup_key = (image_handle, view_desc);
        if let Some(&existing_handle) = self.image_view_lookup.get(&lookup_key) {
            return existing_handle;
        }

        let image = self.image_pool.get(image_handle).expect("Invalid image handle");
        let image_view = GfxImageView::new(image.handle(), view_desc, name);
        let image_view_handle = self.image_view_pool.insert(image_view);

        // 更新 lookup 表
        self.image_view_lookup.insert(lookup_key, image_view_handle);

        // 更新 image -> views 缓存
        self.image_to_views.entry(image_handle).unwrap().or_default().push(image_view_handle);

        image_view_handle
    }

    /// 获取 ImageView 资源引用
    pub fn get_image_view(&self, handle: GfxImageViewHandle) -> Option<&GfxImageView> {
        self.image_view_pool.get(handle)
    }
}
